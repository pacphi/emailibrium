# qe-court record — postgres-support, phase 4 (CI proves PostgreSQL; one-flag backend selection)

- **Delivery:** `git diff develop...HEAD` on branch `autopilot/postgres-support/phase-4` — 11 files at
  filing time (399 insertions), plus the remediation commit `9adde64`. PR #260.
- **Convened:** 2026-08-06, per `pipeline.court: auto` + phase 4 ∈ `risk_phases`.
- **Config:** `.claude/skills/qe-court/config.json` (2-vendor panel; `writerIsNeverJuror`,
  `blindFiling`, `minDistinctVendors: 2`, `overturnDepth: 2`).

## Panel

| Seat                        | Route                                  | Status                              | Charges |
| --------------------------- | -------------------------------------- | ----------------------------------- | ------- |
| prosecutor.brutal-honesty   | claude-code (agent, sonnet)            | filed (late, after remediation)     | 4       |
| prosecutor.mutation         | claude-code (agent, isolated worktree) | filed (8 mutations applied)         | 3       |
| prosecutor.security-scanner | codex                                  | filed                               | 1       |
| prosecutor.codex-review     | codex                                  | filed                               | 2       |
| prosecutor.devils-advocate  | codex                                  | filed                               | 9       |
| prosecutor.sherlock         | codex                                  | filed (6 claims verified, 1 charge) | 1       |
| defense                     | claude-code                            | this record's dispositions          | —       |
| jury                        | codex                                  | see Verdict                         | —       |

All four codex seats completed this time (contrast phase 3, where the full-diff size stalled them):
the delivery is small and each seat was given an explicit numbered file list with per-step progress
reporting, per the phase-2/3 lesson. `minDistinctVendors: 2` holds; both grading roles are codex,
and the writer (Claude) never judged.

## The three defects the delivery itself found

Recorded here because they are the substance of the phase, and because a reader a year from now
should not have to reconstruct why a "CI and docs" phase touched `config.rs`:

1. **figment silently dropped the variable.** `Env::prefixed("EMAILIBRIUM_").split("_")` splits on
   every underscore, so `EMAILIBRIUM_DATABASE_URL` became nested `database.url` — a key
   `VectorConfig`'s flat `database_url` field never matches, and serde discards unknown fields.
   Proven with a failing test before the fix (`left: "sqlite:emailibrium.db?mode=rwc"`).
2. **entrypoint.sh resolved the secret to a name nobody reads.** `/run/secrets/database_url` →
   `DATABASE_URL`; grep confirms nothing under `backend/src` reads that unprefixed name.
3. **Nothing reported which backend connected.** A green smoke run proved the stack came up, not
   what it came up on — which is exactly how it "tested PostgreSQL" for its whole life.

## Empirical findings recorded as evidence (sherlock charge Sh1)

The `required: false` claim in `docker-compose.yml` is load-bearing and was asserted in a comment
with no in-repo reproduction. The experiment, run on Docker Compose v5.1.2 before the change was
written, is recorded here so the claim is checkable rather than trusted:

- `depends_on: postgres: {condition: service_healthy}` **without** `required: false`, `postgres`
  behind `profiles: ["postgres"]`, no profile active →
  `docker compose config` exits non-zero with
  `service "backend" depends on undefined service "postgres": invalid compose project`.
- **With** `required: false`, same conditions → `config` and `up --wait` both succeed, and with
  `--profile postgres` active the backend still waits for postgres to report healthy.
- With the profile active but postgres **failing** to start, the backend is no longer held back:
  Compose logs `optional dependency "postgres" failed to start` and proceeds. This is the accepted
  cost, recorded as `pl-postgres-depends-required-false-untested`.
- Separately: a plain `docker compose down` does **not** remove a profile-gated container
  (`docker compose ps` lists it either way — the asymmetry is easy to miss). This is why the
  teardown recipes now pass `--profile postgres`.

## Charges and dispositions (20 filed)

Remediation commit: `9adde64`. Every disposition below was re-verified by the full suite
(15 binaries, 0 failed) plus a full run against a live `postgres:16-alpine` (1087 + 111 + the
integration binaries, 0 failed, all three `postgres_*` round-trip tests RUNNING rather than skipping).

### mutation — 8 mutations applied, 3 survived

Baseline 26 passed / 0 failed on the targeted set; 1303 passed / 0 failed full suite.

1. **M6 [MEDIUM-HIGH] `postgresql://` was entirely unexercised.** Deleting that half of ADR-033's
   documented selector from `Database::connect` left the **whole suite green** — a deployer using the
   documented spelling would silently have landed on SQLite, the exact failure class this pipeline
   exists to eliminate. REMEDIATED (`9adde64`): scheme check extracted to `selects_postgres()`
   (`connect` itself cannot be tested without a live server) and all four cases pinned, near-misses
   included.
2. **M7 [MEDIUM-HIGH] the `backend_name()` → startup-log wiring is unpinned.** Replacing the call
   site with a re-parse of `config.database_url` — the anti-pattern the doc comment explicitly
   forbids — survived the unit suite **and both smoke legs**, because config and reality agree in
   both. PARTIALLY REMEDIATED + RECORDED (`pl-startup-log-wiring-unpinned`):
   `startup_backend_line()` pins the exact string smoke greps for, so a rename now fails in seconds
   instead of quietly reducing that CI leg to a no-op grep. The stronger property — that the line
   follows the CONNECTION, not the config — still has no executable check; it needs a boot-level test
   where the two deliberately disagree.
3. **M2 [LOW] provider merge order unpinned.** Swapping the two env providers was invisible to the
   suite. REMEDIATED (`9adde64`): pinned by the one case where order IS observable — a struct-shaped
   `EMAILIBRIUM_STORE` set over its own child must fail loudly rather than silently resolve.
   Verified by re-applying the order swap and watching the new test go red.
4. Mutations 1, 3, 4, 5, 8 were all KILLED (deleting the new provider; removing `jail.clear_env()`
   under an exported `EMAILIBRIUM_DATABASE_URL`; swapping and then flattening the `backend_name`
   arms; deleting the _split_ provider) — the last confirming the paired assertion genuinely pins
   both providers, as the test's own doc claims.

**Seat hygiene note:** this seat deleted and recreated the vendored `ruvector` submodule directory
inside **its own git worktree** to work around an empty checkout. Verified afterwards that the main
tree's submodule is untouched and clean at `53f0419`. Nothing was committed or pushed from the
worktree.

### devils-advocate — 9 completeness gaps

1. **[CRITICAL] The PostgreSQL recipes did not select PostgreSQL.** `docker-up-postgres` enabled the
   profile — which starts the container — while the URL is the actual backend selector, so the app
   stayed on SQLite next to an idle database: failure that looks like success. REMEDIATED
   (`9adde64`): both `-postgres` recipes derive the URL from the same `db_password` secret the
   service reads; an explicit `EMAILIBRIUM_DATABASE_URL` still wins. All three branches
   (derive / explicit / missing-secret error) exercised.
2. **[MAJOR] Native mode is only a help claim; `dev-llm` omitted.** PARTIALLY ANSWERED: help now
   names `dev-llm` and direct `cargo test`/`cargo run`. The native path against PostgreSQL is in
   fact the most-exercised one in this delivery — `rust-test-postgres` and the local reproduction
   are both native `cargo` runs against a live server, not Docker.
3. **[MAJOR] `docker-compose.dev.yml` unmodified though listed in `touches`.** REMEDIATED
   (`9adde64`): the dev overlay documents the toggle at its `postgres` block, and the overlay was
   validated with **and** without the profile (`config --services` both ways).
4. **[MAJOR] The CI job never proves the shipped server path.** ANSWERED, by design: `rust-test-postgres`
   proves the library/repository layer on PostgreSQL; smoke's second leg boots the real release image
   against PostgreSQL, runs startup migrations, and answers its health endpoint. Neither alone is
   sufficient, which is why both exist.
5. **[MAJOR] Smoke proves a label, not database functionality.** REMEDIATED (`9adde64`): the
   PostgreSQL leg now asserts successful rows in `_sqlx_migrations` — a container that connected but
   could not write would otherwise pass a health check and print a correct log line. Residual (no
   authenticated application-data round trip) RECORDED as `pl-smoke-no-app-data-roundtrip`.
6. **[MAJOR] The required local reproduction is absent from the repo.** REMEDIATED (`9adde64`):
   `.github/workflows/ci.yml` carries the throwaway-container reproduction, including how to confirm
   the `postgres_*` tests RAN rather than skipped.
7. **[MAJOR] Execution evidence is only declarative.** ANSWERED: the DoD is verified by the gate,
   whose outputs are in this record and the PR body; DoD line 4 explicitly defers to the PR's real
   check results rather than to a claim in a file.
8. **[MAJOR] Profile wiring deliberately weakens PostgreSQL startup ordering.** ACCEPTED as a
   documented deviation, RECORDED `pl-postgres-depends-required-false-untested`. The DoD's "wired the
   same way it is today" holds for the service definition (byte-identical plus `profiles:`) but NOT
   for the dependency condition, and this record says so rather than letting the phrase pass.
9. **[MINOR] Dev teardown asymmetric.** ANSWERED: `docker-down` uses `COMPOSE`, not `COMPOSE_DEV`,
   but the Compose project identity is the directory, and the dev overlay adds only ports/targets to
   existing services — no service or resource exists that a plain `down` would miss. The real gap was
   the profile, and that is fixed.

### codex-review — 2 charges

1. **[MAJOR] `docker-restart` could restart a PostgreSQL deployment without PostgreSQL.** Teardown
   activated the profile while startup always ran the SQLite-only `docker-up`. REMEDIATED
   (`9adde64`): split into `docker-restart` / `docker-restart-postgres`, with the fact that restart
   cannot be backend-preserving (`down` destroys the containers that knew their URL) documented.
2. **[MINOR] The two providers DO conflict for mixed scalar/object keys, so "compose rather than
   conflict" was false.** REMEDIATED (`9adde64`): the comment now describes the collision exactly —
   the unsplit provider merges last and writes a scalar over the split provider's dict, and
   extraction fails with a type error — and argues why a loud failure is the right outcome. Pinned by
   the new test (see mutation M2).
3. Clean on: configured-variable regression (no existing variable changes meaning), the new tests'
   ability to fail, entrypoint ordering/POSIX behavior under `set -e`, `backend_name` exhaustiveness,
   and the teardown recipes.

### security-scanner — 1 charge

1. **[MINOR] The service container publishes `5432:5432` on the runner** with a known throwaway
   password. ACCEPTED, RECORDED `pl-ci-postgres-port-published`: GitHub's `services:` syntax cannot
   restrict the bind interface, and the job's steps run directly on the runner, so a published port
   is how they reach it at all. Safe on ephemeral GitHub-hosted runners; the record names the fix and
   the condition (a shared self-hosted `HEAVY_RUNNER`) that would make it worth paying for.
2. Clean on: credential exposure (`main.rs` logs only `backend_name()`; `entrypoint.sh` emits only
   variable names), hardcoded credentials, Actions injection, override precedence, and network
   segmentation (postgres stays on `db-internal`; the profile changes activation, not reachability).

### sherlock — 6 claims verified, 1 charge

VERIFIED: the figment mechanism as described; that both halves of the two-provider claim hold; that
nothing under `backend/src` reads unprefixed `DATABASE_URL`; that the startup log cannot include the
URL (`backend_name` returns only two literals); and that the phase-3 court-record edit is
formatting-only — table alignment, equivalent emphasis delimiters, and code spans around
underscore-bearing identifiers — changing no charge disposition, tally, or verdict.

1. **[MINOR] The Compose `required: false` claim was unrecorded.** REMEDIATED by this record's
   "Empirical findings" section above, which carries the commands, conditions, and exact error.

### brutal-honesty — 4 charges + 1 probe completed by the defense

Filed late (after the remediation commit), and it landed the sharpest charge of the court.

1. **[CRITICAL] The live operator docs still tell readers PostgreSQL does not work.**
   `docs/deployment-guide.md:236-241` says SQLite is "currently the **only** database the backend can
   actually connect to" and instructs readers: "Do not set `EMAILIBRIUM_DATABASE_URL` to a
   `postgres://` URL; the backend will fail to connect." `docs/configuration-reference.md:53` agrees.
   ACCEPTED-AS-SCOPED, not remediated here: **phase 5 is exactly this work** — it owns both files,
   `depends_on: [4]`, and carries the DoD `grep:absent: "planned, not implemented"`. Folding the
   rewrite into a CI phase is the scope-creep the discovered-work contract exists to prevent. Two
   things keep this honest rather than convenient: the contradiction is **pre-existing**, not created
   here (`backend/Cargo.toml:37` has enabled the sqlx `postgres` feature since phase 0, so the doc was
   already false), and `develop` is an integration branch — no user reads these docs until the
   base→trunk PR, which is gated behind phase 5 landing. If phase 5 were ever dropped, this charge
   becomes a release blocker, and this paragraph is the record of that.
2. **[MAJOR] This change's own "known gap" comment named the mildest example.** It disclosed only
   `store.qdrant_url`, while the same unreachable shape silently swallows four env vars that
   `docs/configuration-reference.md` advertises as the canonical examples of the whole mechanism.
   REMEDIATED as to the claim (the comment now states the real blast radius and names the candidate
   fix); the underlying bug is RECORDED as `pl-nested-flat-env-keys-dropped`, not fixed, because the
   fix changes the env contract for all of them at once. **The defense verified the charge and found
   it understated:** `encryption.master_password` is the worst case — configuration-reference.md:133
   says "Never set in config files; use env var or Docker secret", `encryption.rs:97` reads only the
   config field, and `security.encryption_key_env` (which would imply a direct env read) is consumed
   **nowhere** in `backend/`. An operator following the documented, security-sanctioned path gets
   `Master password required` at startup.
3. **[MAJOR] `docker-compose.yml` wires a dead env var three lines from the one this PR fixed.**
   `EMAILIBRIUM_STORE_QDRANT_URL` never reaches the app, in a file and an environment block this PR
   was already editing. REMEDIATED (disclosure comment at the point of use — overriding `QDRANT_URL`
   changes nothing, and the file now says so instead of looking functional).
4. **[MINOR] The Compose Services doc table was falsified by this change.**
   `docs/deployment-guide.md:209` listed `postgres` with no profile note while the very next row says
   `qdrant` is "profile-gated". Accurate before this PR; not after. REMEDIATED — the one row this
   change falsified is corrected, deliberately without touching the Database Strategy narrative that
   phase 5 owns.
5. **Probe left unfinished:** whether `rust-test-postgres` is a _required_ status check (no `gh` in
   that seat's environment). COMPLETED BY THE DEFENSE, and the answer matters:
   `gh api repos/pacphi/emailibrium/branches/develop/protection` → **404, "Branch not protected"**.
   No check is GitHub-required on `develop`. "CI actually proves PostgreSQL works" therefore holds for
   autopilot's merges — its playbook refuses a red or check-less PR — but nothing mechanically stops a
   human merging past a failed PostgreSQL job. RECORDED `pl-develop-not-branch-protected`.

The seat additionally probed and found **clean**, several empirically: entrypoint shell semantics
under `sh`/`set -e`, figment precedence and collision risk (confirming no `deny_unknown_fields`), the
`required: false` tradeoff, and — by running a live `--profile postgres` up/ps/teardown cycle — that
smoke's negative assertion is real, i.e. `docker compose ps --all --services` does detect a rogue
postgres container without the profile flag.

## Verdict

_(recorded below by the jury seat)_
