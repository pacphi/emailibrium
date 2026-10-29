# Live IMAP Conformance

Two tools check emailibrium's IMAP behavior against a real mailbox. Both run on your own machine; neither runs in CI.

| Tool | Writes to the mailbox | What it tells you |
| --- | --- | --- |
| `scripts/imap-probe.sh` | No | Which move strategy (`MOVE`, `UIDPLUS`) and keyword support (`PERMANENTFLAGS`) the server offers |
| `backend/tests/live_imap_conformance.rs` | Yes, inside scratch folders it creates | Whether labels, undo, and archive round-trip correctly on that server |

Use a dedicated test account for the conformance test. Use an app password, never your main password.

## 1. Probe a mailbox (read-only)

```bash
scripts/imap-probe.sh imap.gmail.com 993 you@gmail.com
```

It asks for the app password without showing it, logs in, sends `CAPABILITY`, opens INBOX read-only with `EXAMINE`, and logs out. It writes the server's responses to `imap-probe-<host>-<time>.txt` in the current folder. The password never appears in that file.

Other providers:

```bash
scripts/imap-probe.sh imap.mail.me.com 993 you@icloud.com
scripts/imap-probe.sh imap.fastmail.com 993 you@fastmail.com
scripts/imap-probe.sh imap.mail.yahoo.com 993 you@yahoo.com
scripts/imap-probe.sh 127.0.0.1 1143 you@proton.me --starttls
```

Outlook and Microsoft 365 refuse password login (`LOGINDISABLED`), so the probe can't reach them.

What to look for in the output file:

- `p1 OK` — login worked.
- The `* CAPABILITY` line after `p2`: `MOVE` and/or `UIDPLUS`.
- `[PERMANENTFLAGS (...)]` after `p3`: `\*` means the mailbox keeps custom keywords.

## 2. Run the conformance round trip (writes)

Set these in the terminal you run the test from:

| Setting | Example |
| --- | --- |
| `EMAILIBRIUM_TEST_IMAP_HOST` | `imap.gmail.com` |
| `EMAILIBRIUM_TEST_IMAP_PORT` | `993` |
| `EMAILIBRIUM_TEST_IMAP_ENCRYPTION` | `ssl` (implicit TLS), `tls` (STARTTLS), or `none` |
| `EMAILIBRIUM_TEST_IMAP_USERNAME` | `you@gmail.com` |
| `EMAILIBRIUM_TEST_IMAP_PROFILE` | `generic`, `proton_bridge`, or `outlook_imap` |
| `EMAILIBRIUM_TEST_IMAP_PASSWORD` | the app password (or `EMAILIBRIUM_TEST_IMAP_OAUTH_TOKEN` for an OAuth access token — set exactly one) |
| `EMAILIBRIUM_TEST_IMAP_ALLOW_WRITES` | `yes` |

Then, from `backend/`:

```bash
cargo test --features vectors --test live_imap_conformance live_imap_round_trip -- --ignored --nocapture
```

A missing setting fails the run; it never passes silently.

What the test does:

1. Creates `Emailibrium-Conformance-<id>` and `Emailibrium-Conformance-Archive-<id>`, and stops if the first one isn't empty.
2. Appends one synthetic message there, with subject `emailibrium conformance test (safe to delete)`.
3. Applies `Kind/EmailibriumTest<id>` and `State/EmailibriumTest<id>`, undoes them, and checks the folders match their starting state.
4. Archives the message into the scratch archive folder, moves it back, and checks again.
5. Fails if the message ever appears in INBOX.
6. Deletes exactly the folders it created. On Gmail it first copies the message to Trash, because deleting a Gmail label folder leaves the message in All Mail.

The output names the move strategy and label path the server used, and lists any folder it could not delete. If the server created an empty parent folder (for example `Kind`), the output says so and leaves it in place.

On Gmail, one copy of the synthetic message stays in Trash until Gmail empties it.
