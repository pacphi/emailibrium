# Integration Court — `keyboard-shortcuts`

**Delivery:** `develop` vs `main` (merge-base `0319597`), 6 commits, 2262-line diff — 5 feature
phases + 1 cross-phase optimization pass, all individually gated and merged.

**Convened:** 2026-08-03. Panel per `.claude/skills/qe-court/config.json`: prosecutors
devils-advocate, brutal-honesty, sherlock, security-scanner, mutation, codex-review.
**Jury/overturn round:** skipped by explicit human decision (see below) — this record stops at
the prosecution phase; the human judge renders the verdict directly from the filed evidence.

## Panel status (disclose, never paper over)

| Seat             | Provider             | Status                                                                                                                                            |
| ---------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| brutal-honesty   | claude-code (Sonnet) | **Filed** — 12 charges, 1 blocker                                                                                                                 |
| mutation         | claude-code          | **Filed** — 7 charges (empirically verified: 15 mutations tried, 10 survived)                                                                     |
| sherlock         | codex                | **Filed** — 11 charges, 1 blocker (4 independently re-verified by the convening agent)                                                            |
| codex-review     | codex                | **Filed** — 12 charges (engagement independently confirmed: real thread ID, all cited file:line references validated against actual file lengths) |
| security-scanner | codex                | **SKIPPED** — `mcp__codex__codex` hard-hung 30 min, zero output. Not a clean pass.                                                                |
| devils-advocate  | codex                | **SKIPPED** — hung 36+ min, force-stopped by the human judge before any output. Not a clean pass.                                                 |

2 of 6 seats unfilled. Both skipped seats were codex-routed; codex responded successfully for
the other two codex seats (sherlock, codex-review), so this reads as intermittent unreliability
under this session's load, not a total outage. **Vendor diversity for this run: Claude
(brutal-honesty, mutation) + GPT/codex (sherlock, codex-review) — the `minDistinctVendors: 2`
invariant is met on the seats that filed**, but the security lens specifically is unexamined by
any adversarial reviewer (only the pipeline's own phase-level Tier-3 passes touched it).

## Independently confirmed by the convening agent (not just asserted by a prosecutor)

- `bash scripts/audit/extract-shortcuts.sh` emits exactly 7 keys (`c e escape f r shift+#
shift+?`) against 13 documented in `docs/user-guide.md`. Re-run yourself: the command is
  deterministic and takes under a second.
- `docs/user-guide.md:223-226` currently reads: "This table is verified against the app's
  actual registered shortcuts by `bash scripts/audit/extract-shortcuts.sh`..." — that sentence
  is false as shipped.
- `git status`/`git diff --stat` confirmed clean after all prosecutors finished (no stray
  scratch files from concurrent blind review left in the tree).

## Charges, deduplicated and clustered by underlying issue

Severity is the convening agent's synthesis, not a formal jury score. "Corroboration" counts
independent prosecutor seats that reached the same finding by their own separate analysis.

### BLOCKER — recommend fixing before merge into `main`

**B1. Verification script silently broken; docs ship a false claim.** _(Corroborated 3x:
brutal-honesty #1/#2, codex-review #8, sherlock #2/#3 — plus directly confirmed by the
convening agent.)_ The optimization pass's `metaOrCtrl(...)` spread replaced literal
`'cmd+k': toggle` object entries with computed template-string keys built _inside_
`useKeyboard.ts`'s `metaOrCtrl` function body. `extract-shortcuts.sh`'s regex only recognizes
literal quoted/bare keys in the _calling_ file's source text, so it can no longer see `cmd+k`,
`ctrl+k`, `cmd+,`, `ctrl+,`, `cmd+shift+a`, `ctrl+shift+a` — 6 of 13 real shortcuts. Nobody
caught it because the optimization pass's Tier-3 review ran typecheck/lint/test/build but
never re-executed the shell script, and the script isn't wired into CI (`docs-accuracy.yml`
path-filters on `scripts/audit/**` but never invokes this specific script — confirmed by
brutal-honesty and independently by sherlock).
_Fix shape:_ either make the extraction script resolve `metaOrCtrl`-generated keys (parse its
own definition and expand call sites), or have it actually diff bidirectionally against
`docs/user-guide.md` and fail loudly instead of only printing keys with no comparison at all
(sherlock #3: the script never opens the doc file, ever). At minimum, the false verification
sentence in the doc must not ship as-is.

**B2. Focus can leave Compose's autofocused field and land on non-guarded elements, letting
destructive email shortcuts fire behind the modal.** _(sherlock #1, corroborated by
codex-review #3.)_ `useKeyboard`'s editable-field guard recognizes only
`INPUT`/`TEXTAREA`/`contenteditable === "true"` (exact string match — misses
`contenteditable=""`, `contenteditable="plaintext-only"`, and any focusable descendant of a
contenteditable host). ComposeEmail's autofocus only protects the _initial_ focus; the modal
contains a `<select>` (From) and multiple buttons. Once focus moves off the initial input,
`E`/`Shift+#`/`R`/`F`/`C` reach the still-mounted email shortcuts underneath.
_Repro:_ open an email, press `C`, Shift+Tab from the To field to the From select, press `E` —
the underlying email archives while Compose is still open.

**B3. Cmd+Shift+A then E/# is an unconfirmed, uncounted, irreversible bulk-destroy path.**
_(brutal-honesty #11.)_ This is the one charge that's a genuine **judgment call, not a
factual dispute** — phase 2's own Tier-3 review already surfaced this and the pipeline
recorded a parking-lot decision (`pl-bulk-shortcut-no-confirm`) reasoning it wasn't a new risk
class since manual multi-select + the existing bulk-action buttons already permitted the same
end state. Brutal-honesty directly challenges that reasoning: requiring N deliberate checkbox
clicks **was** the safety mechanism; collapsing it to one keystroke removes it, and "reaching
the state faster" reframes a regression as a feature. Both readings are defensible — this is
squarely a call for the human judge, not something the court can resolve unilaterally.

### HIGH — real, verified gaps; recommend fixing or explicitly deferring in writing

**H1. The duplicate-registration hazard was routed around at specific call sites, not fixed at
the root.** _(codex-review #2, sherlock #5.)_ `useKeyboard` still installs one independent
`window` listener per consumer; `stopPropagation()` does not stop sibling listeners on the same
target. Two consumers registering the same key both fire — phase 0's fix only changed
`CommandCenter.tsx` to stop being one of the two consumers for Cmd+K specifically; the
mechanism itself has no collision detection. _Repro (codex-review):_ open help with `?`, then
open the palette with Cmd+K, press Escape — both stacked overlays close instead of only the
topmost, because both `escape` registrations fire on one keypress.

**H2. Select-all silently omits unloaded pages _and_ silently includes collapsed/invisible
grouped rows.** _(sherlock #7, extends the pipeline's own already-logged `filteredEmails`
pagination limitation with a new, more severe finding.)_ In grouped view with all groups
collapsed, `Cmd+Shift+A` selects every loaded email even though zero rows are visually shown
as checked — combined with B3, a user can bulk-destroy emails they never saw were selected.

**H3. Archive/Delete shortcuts bypass view-specific action rules.** _(codex-review #7.)_
`EmailActions` deliberately hides Archive in Spam/Trash and gates permanent deletion behind a
confirmation dialog in Trash. The keyboard shortcuts call the generic archive/delete mutations
unconditionally, producing actions the visible toolbar in that view doesn't even offer.

**H4. The help panel's core "sourced from the live registry" claim is proven with a
production-impossible test, and the real panel shows 4 of 13 rows.**
_(3-way: brutal-honesty #3/#4, sherlock #4, implied by codex-review #1.)_ `ShortcutHelpPanel` mounts only inside
`CommandCenter` (`/command-center`); email shortcuts mount only inside `EmailClient`
(`/email`) — sibling routes, never simultaneously mounted (confirmed by brutal-honesty by
mounting the real topology: the panel shows only `Escape`, `Cmd+K`/`Ctrl+K`,
`Cmd+,`/`Ctrl+,`, `Shift+?`). The one test claiming to prove the panel shows email shortcuts
manually co-mounts a synthetic registrant beside the panel — a topology no route ever
produces. 7 of 13 `SHORTCUT_LABELS` entries are dead code in the shipped app.

**H5. Reply/Forward opens the editor without moving focus into it.** _(codex-review #4.)_
`R`/`F` changes `ReplyBox`'s mode/expansion state but never focuses the textarea or
forward-recipient input, so the very next keystroke can hit a different shortcut instead of
being typed as reply text.

**H6. The registry's reference-counting decrement path is unverified and its own test is
self-defeating.** _(mutation #1, empirically proven — 2 independent mutations survived: an
off-by-one in the decrement guard, and removing the increment entirely.)_ The test named for
exactly this ("keeps a shared key registered while at least one consumer still holds it") uses
a consumer with an inline object-literal `ShortcutMap`, so its own effect re-runs on rerender
and masks the broken decrement by accidentally re-registering. Real-world impact: one
short-lived remount could silently clear a key another mounted consumer still needs, making the
help panel wrongly omit an actually-live shortcut — the exact drift phase 3 was built to
prevent.

**H7. Archive/Delete/select-all tests assert an injected mock, never the real wiring.**
_(3-way: brutal-honesty #6, codex-review #12, sherlock #8.)_ All three prosecutors
independently found the same gap: `useEmailShortcuts.test.tsx` passes `vi.fn()` as
`onArchive`/`onDelete`/`onSelectAll` and asserts the hook calls whatever it's handed. Nothing
tests that `EmailClient.tsx` actually wires `handleThreadArchive` (the real click-driven
function) to it — swapping the wiring at the `EmailClient` call site would leave every one of
these tests green.

### MEDIUM — worth fixing, none independently ship-blocking

- **M1.** stopPropagation is asserted by name only; the test titled "calls preventDefault and
  stopPropagation on a match" checks just `defaultPrevented` (mutation #2 — empirically
  confirmed by deleting the real call and rerunning; corroborated by sherlock #11).
- **M2.** ReplyBox's stale-draft regression test never asserts the draft was actually cleared,
  only that the UI collapsed — removing the two `setBody('')`/`setForwardTo('')` calls this
  phase added would leave the test green (sherlock #10).
- **M3.** CommandPalette's Escape "migration" claim is inaccurate: the shared registration
  can't handle Escape while the palette's own autofocused input has focus (the editable-guard
  blocks non-modifier shortcuts there); it only works today because the pre-existing local
  `onKeyDown` handler on the `Command` component was never removed (sherlock #9).
- **M4.** Help panel (`role="dialog"`, `aria-modal="true"`) has no focus trap or focus
  restore-on-close, despite an existing, unused `useFocusTrap` hook in the same codebase
  (codex-review #5).
- **M5.** `event.repeat` is never checked — holding a shortcut key re-fires its handler
  repeatedly (codex-review #10).
- **M6.** `#`/`?` matching assumes a US keyboard layout (both require Shift on US layouts only)
  — a UK layout's dedicated `#` key matches neither registered form (codex-review #11).
- **M7.** `Cmd+,` triggers a full page reload (`window.location.href`) in a client-routed SPA,
  discarding the query cache — the "matches the sidebar's existing pattern" justification is
  accurate but doesn't address that a keyboard shortcut sets a higher expectation of instant
  navigation (brutal-honesty #10).
- **M8.** No test exists in an actual browser; all ~1000 new test lines run in jsdom, where
  `preventDefault()` has no real browser default to prevent. `Cmd+Shift+A` and `Cmd+,`
  collide with real Chrome/Firefox/macOS-level shortcuts that a page-level `preventDefault`
  cannot reliably win against, and nothing in this delivery's process ever opened a real
  browser to check (brutal-honesty #9). `CLAUDE.md` calls for browser verification on
  frontend changes.
- **M9.** `formatShortcutKey`'s multi-character branch is untested (all 5 unit tests use
  single-char keys) — mutating it to drop the multi-char capitalization renders "Escape" as
  "scape" in the live help panel with nothing failing (mutation #6).
- **M10.** Both sort calls in `buildShortcutRows` are untested because every assertion re-sorts
  the actual result before comparing it, discarding exactly the ordering property the
  production code computes (mutation #7).
- **M11.** `alt` as a guard-bypassing modifier is asserted only in a _matching_ test, never in
  a guard/editable-field test — latent today (no `alt+`-only shortcut ships), but unverified
  for the next one that does (mutation #5).
- **M12.** `EmailClient.tsx`'s `handleThreadArchive` now clears `selectedEmailId` after
  archiving — a real behavior change to the existing **mouse-driven** thread-action-bar
  button (not just the new keyboard shortcut), shipped with zero dedicated test, under a
  PR titled as a keyboard feature (brutal-honesty #7).

### LOW

- **L1.** `frontend/apps/web/src/shared/test-utils/press.ts` imports the `@testing-library/react`
  devDependency from inside `src/`, with no `.test.`/`__tests__` path marker — harmless today
  (Vite tree-shakes it), but a bundling-hygiene footgun (brutal-honesty #12).
- **L2.** "First match wins" is asserted by a test whose two entries (`k` and `cmd+k`) can never
  both match the same real event under exact-modifier matching, so the early-`return` it claims
  to verify is never actually exercised (mutation #3).
- **L3.** The conditional-`escape`-registration design intent (only register while open, to
  avoid swallowing every bare Escape press while closed) has no test that actually distinguishes
  "registered and no-op" from "not registered" — both existing "no-op while closed" tests check
  only the store value, which is identical either way (mutation #4).

## Defense (writer's case, filed blind before the prosecution — see `defense-case.md` in the

session scratchpad; summarized here for the record)

Every phase individually passed Tier 1 (format/lint/build/test) + Tier 2 (DoD, cited evidence)

- Tier 3 (independent adversarial review) before merging — across the 5 phases and the
  optimization pass, 8 real bugs were found and fixed _before_ this integration seat, all
  disclosed rather than silently absorbed (2 phase-0 bugs, 2 phase-1, 1 phase-2, 1 phase-3, 2
  phase-4/optimization). Every deliberate scope decision (touching files outside a phase's
  stated allowlist, deferring a finding, skipping a migration) was disclosed in commit messages,
  PR bodies, and `.autopilot/discovered/keyboard-shortcuts.jsonl` — nothing was quietly expanded
  or buried. 288/288 frontend tests and the full backend suite were green at every phase
  boundary. The one already-known open question (Cmd+K's route-scoping) was flagged from phase 0
  onward, not discovered here for the first time.

**What the prosecution shows the defense missed:** per-phase review depth was consistently
shallower than the feature's actual integration-time risk surface — largely because each
phase's Tier-3 pass verified _that a change works in isolation_, not _that the integrated,
route-level, focus-management, and cross-view reality behaves as claimed_. The verification
script regression (B1) is the starkest example: a phase whose entire purpose was proving the
docs table "can be checked instead of trusted" broke that exact guarantee one commit later,
undetected, because the check that would have caught it was never actually run as part of
verifying the change that broke it.

## Recommendation to the human judge

This record stops here — no jury verdict, no overturn round (skipped by explicit user
decision given codex's ~50% availability during this run: 2 of 4 codex-routed seats hung with
zero output). The evidence gathered from the 4 filed seats is extensive and substantially
cross-corroborated (3-way agreement on H4 and H7; independently confirmed via direct
reproduction on B1, H1, H6, M1), which is why the convening agent recommends treating this as
sufficient signal to act on without a formal jury, rather than re-attempting the hung seats.

The convening agent's synthesis-level read: **REMAND, not SHIP.** B1 (false, broken
verification claim) and B2 (real destructive-shortcut leak through Compose) are concrete,
reproducible defects that should be fixed before this reaches `main`, independent of any
judgment call. B3 is a genuine values question for the human judge — not a factual dispute the
court can resolve — and the human's ruling on it should be explicit rather than left as a
standing parking-lot note contested by an adversarial reviewer.

## Remediation (2026-08-03, ordered by the human judge)

The human judge ruled REMAND and ordered every filed finding fixed. Disposition, on branch
`autopilot/keyboard-shortcuts/court-remand`:

- **B1 fixed.** `extract-shortcuts.sh` now expands `metaOrCtrl(...)` calls (and bracket
  assignments), parses the doc table, compares bidirectionally, and exits non-zero on drift —
  negative-tested (a seeded doc mutation makes it fail both directions). Wired into
  `docs-accuracy.yml` as a third job with `frontend/apps/web/src/**` + `docs/user-guide.md`
  path filters. The doc's verification sentence is true again.
- **B2 fixed.** Email shortcuts are fully unregistered while any modal (compose, move,
  bulk-confirm) is open (`enabled` flag on `useEmailShortcuts`); the editable-field guard now
  also covers `<select>` and every contenteditable form. Regression-tested at the EmailClient
  level (compose open → `e`/`#` cannot reach the mutations).
- **B3 fixed (human ruling: charge valid).** Bulk (>1) archive/delete parks in a count-bearing
  confirmation dialog before any mutation, for keyboard and action-bar paths alike; Cancel is
  the focused default.
- **H1 fixed at the root.** `useKeyboard` now uses one shared window listener over a
  registration stack, newest-first, one handler per keypress — stacked overlays close one at a
  time, and the phase-0 double-registration bug class is structurally gone.
- **H2 fixed.** Grouped-mode select-all covers only visible rows (built on the same
  `flattenGroups` the list renders); a count toast always reports what was selected, with an
  explicit note when unloaded pages exist.
- **H3 fixed.** Trash view: `#` routes to confirmation-gated permanent delete, `e` is
  unregistered; spam view: `e` unregistered — matching the visible action bar, wiring-tested.
- **H4 fixed.** Palette, help panel, and `?`/`Cmd+,` registration moved to the app shell
  (`Layout.tsx`); the panel now lists each route's live registry on every page. The
  impossible-topology test is replaced by tests that match the real (shell-sibling) topology.
- **H5 fixed.** `ReplyBox` focuses the body textarea (reply) or recipient input (forward) on
  expand; focus-asserted in tests.
- **H6 fixed.** Refcount decrement pinned by a test using module-stable maps that cannot
  accidentally re-register; both surviving mutants from the mutation report now die.
- **H7/M12 fixed.** New `EmailClient.test.tsx` (12 tests) renders the real component with the
  real hook chain and spies mutations at the module boundary — e/#/select-all wiring,
  selection-cleared-after-archive, bulk confirm, trash routing, compose suppression.
- **M1–M11, L1–L3 fixed** as filed: stopPropagation spied; draft-clear asserted on re-open;
  CommandPalette's local Escape handler removed (escape is guard-exempt, dispatcher handles it
  with the input focused); help panel uses `useFocusTrap` (focus in on open, restored on
  close); `event.repeat` ignored; `#`/`?` layout-independent (registered bare, shift-agnostic
  matching for symbol keys); `Cmd+,` navigates via the router (no page reload); browser-reserved
  combo caveat documented (real-browser E2E remains a parking-lot item —
  `pl-no-real-browser-verification`); `formatShortcutKey` multi-char and `buildShortcutRows`
  exact-order tests added; `press.ts` moved outside `src/` behind a `@test-utils` alias;
  first-match-wins pinned via the cmd+k/meta+k alias collision; conditional escape registration
  asserted at the registry level.

Gate: format ✓, eslint ✓, typecheck ✓, frontend 331/331 ✓ (43 net new tests), build ✓,
backend 1177/0 ✓, `extract-shortcuts.sh` ✓ (13 keys, bidirectional match). Parking-lot items
`pl-cmdk-not-truly-global`, `pl-bulk-shortcut-no-confirm`, `pl-select-all-no-partial-indicator`
resolved by this remediation; `pl-no-real-browser-verification` opened for the M8 residual.
