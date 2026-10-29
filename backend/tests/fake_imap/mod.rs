//! Minimal in-process IMAP server for integration tests (mail-agent plan,
//! Milestone 0; extended for Milestone 6's provider-adapter conformance
//! suite).
//!
//! This is not a conformant IMAP4rev1 server — it implements only the exact
//! command shapes emailibrium's real `ImapProvider` (built on `async-imap`)
//! sends: `LOGIN`, `AUTHENTICATE XOAUTH2`, `SELECT`/`EXAMINE`, `CREATE`,
//! `UID SEARCH` (including `HEADER Message-ID "<id>"`), `UID FETCH`,
//! `UID COPY`, `UID STORE`, `EXPUNGE`, `LOGOUT`. It never validates
//! credentials (LOGIN or XOAUTH2) — the point is to exercise the real
//! client code against a real (if tiny) wire-protocol server, not to test
//! auth failure paths (those are covered by the config-validation unit
//! tests in `src/email/imap.rs`).
//!
//! It tracks real per-UID `\Seen` state across connections: a fetch item
//! without `PEEK` (`RFC822`, `BODY[]`, `BODY[HEADER]`) sets `\Seen` as a side
//! effect, matching real IMAP server behavior (RFC 3501 §6.4.5); a `PEEK`
//! variant never does. This is what proves/disproves "scanning marks nothing
//! read" for the real client code, not a reimplementation of it.
//!
//! Milestone 6 adds real multi-mailbox state (`CREATE`/`UID COPY`/
//! `UID STORE \Deleted`/`EXPUNGE`) so the Proton Bridge (`Labels/*`
//! non-destructive copy) and Outlook (no custom keywords; Kind/Topic
//! redirected to a real folder) adapter paths can be exercised end to end
//! against a real wire protocol, not just unit-tested in isolation.
//! Gmail's `X-GM-EXT-1`/`X-GM-LABELS` extension is still out of scope —
//! Gmail IMAP accounts use `email::gmail` (the REST/OAuth provider), not
//! `ImapProvider`'s generic path this fixture serves.

use std::collections::HashMap;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// One fixture message: an IMAP UID plus its full raw RFC822 text (headers
/// + blank line + body, CRLF line endings).
#[derive(Clone)]
pub struct FixtureMessage {
    pub uid: u32,
    pub raw: String,
    /// Set by `UID STORE ... +FLAGS (\Deleted)`; cleared by `EXPUNGE`
    /// removing the message. Not part of the original milestone-0 fixture
    /// shape, so it defaults to `false` via `new()`/`with_header()`.
    deleted: bool,
}

impl FixtureMessage {
    /// Build a minimal, realistic RFC822 message from header fields. Bodies
    /// are intentionally short — this fixture is about protocol conformance,
    /// not volume.
    pub fn new(
        uid: u32,
        from: &str,
        to: &str,
        subject: &str,
        date_rfc2822: &str,
        body: &str,
    ) -> Self {
        let raw = format!(
            "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date_rfc2822}\r\nMessage-ID: <fixture-{uid}@fake-imap.test>\r\n\r\n{body}\r\n"
        );
        Self {
            uid,
            raw,
            deleted: false,
        }
    }

    /// Insert an extra raw header line (e.g. `Cc` or `List-Unsubscribe`)
    /// before the blank line that separates headers from the body. Added
    /// as a builder method rather than a `new()` parameter so the six
    /// existing milestone-0 call sites don't need updating — milestone 1's
    /// golden-mailbox fixtures are the first to need Cc/List-Unsubscribe.
    // `fake_imap/mod.rs` is included separately, per test binary, via
    // `#[path]` (not a shared library module) — a method or field unused
    // by one binary's particular tests is still live code for another's,
    // so `dead_code` is allowed per-item here rather than silencing the
    // lint for the whole file.
    #[allow(dead_code)]
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        let insert_at = self.raw.find("\r\n\r\n").unwrap_or(self.raw.len());
        self.raw
            .insert_str(insert_at, &format!("\r\n{name}: {value}"));
        self
    }

    /// Extract the raw value of the `Message-ID` header (angle brackets
    /// included), used by `HEADER Message-ID "<id>"` search matching.
    fn message_id(&self) -> Option<String> {
        for line in self.raw.split("\r\n") {
            if let Some(rest) = line
                .strip_prefix("Message-ID:")
                .or_else(|| line.strip_prefix("Message-Id:"))
            {
                return Some(rest.trim().to_string());
            }
            if line.is_empty() {
                break; // end of headers
            }
        }
        None
    }
}

/// One mailbox's message list plus its own UID-allocation counter — real
/// IMAP servers hand out UIDs per-mailbox, which matters once `UID COPY`
/// gives a message a *different* UID in its destination mailbox (the exact
/// behavior `proton_remove_label`'s Message-ID re-lookup depends on).
#[derive(Clone, Default)]
struct Mailbox {
    messages: Vec<FixtureMessage>,
    next_uid: u32,
}

type Mailboxes = Arc<Mutex<HashMap<String, Mailbox>>>;

/// A running fake IMAP server bound to an ephemeral loopback port.
pub struct FakeImapServer {
    pub addr: SocketAddr,
    #[allow(dead_code)] // see with_header's comment: per-binary usage varies
    seen: Arc<Mutex<HashSet<u32>>>,
    #[allow(dead_code)] // see with_header's comment: per-binary usage varies
    mailboxes: Mailboxes,
}

impl FakeImapServer {
    /// Bind `127.0.0.1:0` and start serving a single `INBOX` mailbox seeded
    /// with `messages` to any number of incoming connections in the
    /// background. Returns once the listener is bound; the serving loop
    /// runs on spawned tasks.
    pub async fn spawn(messages: Vec<FixtureMessage>) -> Self {
        let next_uid = messages.iter().map(|m| m.uid).max().unwrap_or(0) + 1;
        let mut map = HashMap::new();
        map.insert("INBOX".to_string(), Mailbox { messages, next_uid });
        Self::spawn_from_map(map).await
    }

    /// Like `spawn`, but seeding an arbitrary set of named mailboxes up
    /// front (milestone 6's conformance suite pre-creates `Labels/*` or
    /// folder mailboxes the way a real account would already have them,
    /// to test COPY/STORE/EXPUNGE against an existing target without also
    /// exercising `CREATE` in the same assertion).
    #[allow(dead_code)] // see with_header's comment: per-binary usage varies
    pub async fn spawn_with_mailboxes(seed: HashMap<String, Vec<FixtureMessage>>) -> Self {
        let mut map = HashMap::new();
        for (name, messages) in seed {
            let next_uid = messages.iter().map(|m| m.uid).max().unwrap_or(0) + 1;
            map.insert(name, Mailbox { messages, next_uid });
        }
        Self::spawn_from_map(map).await
    }

    async fn spawn_from_map(map: HashMap<String, Mailbox>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake IMAP listener");
        let addr = listener.local_addr().expect("fake IMAP local_addr");
        let mailboxes: Mailboxes = Arc::new(Mutex::new(map));
        let seen: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));

        {
            let mailboxes = Arc::clone(&mailboxes);
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                loop {
                    let (stream, _peer) = match listener.accept().await {
                        Ok(pair) => pair,
                        Err(_) => break,
                    };
                    let mailboxes = Arc::clone(&mailboxes);
                    let seen = Arc::clone(&seen);
                    tokio::spawn(async move {
                        let _ = serve_connection(stream, &mailboxes, &seen).await;
                    });
                }
            });
        }

        Self {
            addr,
            seen,
            mailboxes,
        }
    }

    /// Whether the fake server's persisted state currently has `\Seen` set
    /// for this UID — the real-side-effect assertion for the `\Seen`
    /// side-effect fix: a `PEEK` fetch must leave this `false`.
    #[allow(dead_code)] // see with_header's comment: per-binary usage varies
    pub fn is_seen(&self, uid: u32) -> bool {
        self.seen
            .lock()
            .expect("seen mutex poisoned")
            .contains(&uid)
    }

    /// The current (non-expunged) UIDs present in a named mailbox, in
    /// insertion order — the conformance suite's read-back for "did the
    /// label land where the adapter claims."
    #[allow(dead_code)] // see with_header's comment: per-binary usage varies
    pub fn mailbox_uids(&self, name: &str) -> Vec<u32> {
        self.mailboxes
            .lock()
            .expect("mailboxes mutex poisoned")
            .get(name)
            .map(|mb| mb.messages.iter().map(|m| m.uid).collect())
            .unwrap_or_default()
    }

    /// Whether a named mailbox exists at all (distinguishes "exists but
    /// empty" from "never created").
    #[allow(dead_code)] // see with_header's comment: per-binary usage varies
    pub fn mailbox_exists(&self, name: &str) -> bool {
        self.mailboxes
            .lock()
            .expect("mailboxes mutex poisoned")
            .contains_key(name)
    }
}

/// Decode one base64 SASL response line. Never fails closed on malformed
/// input in this fixture — credentials are never checked either way (see
/// module docs) — it only needs to consume the line so the connection
/// state machine advances.
fn decode_sasl_line(line: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(line.trim())
        .unwrap_or_default()
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    mailboxes: &Mailboxes,
    seen: &Arc<Mutex<HashSet<u32>>>,
) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    writer.write_all(b"* OK Fake IMAP ready\r\n").await?;

    let mut current_mailbox: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break; // client closed the connection
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.splitn(3, ' ');
        let tag = parts.next().unwrap_or("*").to_string();
        let cmd = parts.next().unwrap_or("").to_ascii_uppercase();
        let rest = parts.next().unwrap_or("");

        match cmd.as_str() {
            "LOGIN" => {
                // Credentials are never checked — this fixture only proves
                // the real client's wire format round-trips correctly.
                writer
                    .write_all(format!("{tag} OK LOGIN completed\r\n").as_bytes())
                    .await?;
            }
            "AUTHENTICATE" => {
                // SASL XOAUTH2 (milestone 6, Outlook): no real challenge —
                // the client sends its full credential on the first turn
                // regardless of what we send back. One continuation, one
                // client response line, then the final tagged OK.
                writer.write_all(b"+ \r\n").await?;
                let mut resp_line = String::new();
                reader.read_line(&mut resp_line).await?;
                let _ = decode_sasl_line(&resp_line); // never validated, see module docs
                writer
                    .write_all(format!("{tag} OK AUTHENTICATE completed\r\n").as_bytes())
                    .await?;
            }
            "CREATE" => {
                let name = rest.trim().trim_matches('"').to_string();
                mailboxes
                    .lock()
                    .expect("mailboxes mutex poisoned")
                    .entry(name)
                    .or_insert_with(|| Mailbox {
                        messages: Vec::new(),
                        next_uid: 1,
                    });
                writer
                    .write_all(format!("{tag} OK CREATE completed\r\n").as_bytes())
                    .await?;
            }
            "SELECT" | "EXAMINE" => {
                let name = rest.trim().trim_matches('"').to_string();
                let exists = {
                    let mut guard = mailboxes.lock().expect("mailboxes mutex poisoned");
                    // Real servers require the mailbox to already exist;
                    // `connect_and_select`'s configured INBOX is always
                    // seeded, and every other mailbox this fixture serves
                    // is explicitly `CREATE`d first by the client code
                    // under test, matching that requirement.
                    guard.entry(name.clone()).or_default().messages.len()
                };
                current_mailbox = Some(name);
                writer
                    .write_all(format!("* {exists} EXISTS\r\n").as_bytes())
                    .await?;
                writer.write_all(b"* 0 RECENT\r\n").await?;
                writer
                    .write_all(b"* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n")
                    .await?;
                writer
                    .write_all(b"* OK [UIDVALIDITY 1] UIDs valid\r\n")
                    .await?;
                writer
                    .write_all(
                        format!("* OK [UIDNEXT {}] Predicted next UID\r\n", exists + 1).as_bytes(),
                    )
                    .await?;
                let readwrite = if cmd == "EXAMINE" {
                    "READ-ONLY"
                } else {
                    "READ-WRITE"
                };
                writer
                    .write_all(format!("{tag} OK [{readwrite}] {cmd} completed\r\n").as_bytes())
                    .await?;
            }
            "EXPUNGE" => {
                if let Some(name) = &current_mailbox {
                    let mut guard = mailboxes.lock().expect("mailboxes mutex poisoned");
                    if let Some(mb) = guard.get_mut(name) {
                        mb.messages.retain(|m| !m.deleted);
                    }
                }
                writer
                    .write_all(format!("{tag} OK EXPUNGE completed\r\n").as_bytes())
                    .await?;
            }
            "UID" => {
                let mut sub = rest.splitn(2, ' ');
                let subcmd = sub.next().unwrap_or("").to_ascii_uppercase();
                let subrest = sub.next().unwrap_or("");

                match subcmd.as_str() {
                    "SEARCH" => {
                        handle_uid_search(&tag, subrest, &current_mailbox, mailboxes, &mut writer)
                            .await?;
                    }
                    "FETCH" => {
                        handle_uid_fetch(&tag, subrest, &current_mailbox, mailboxes, seen, &mut writer)
                            .await?;
                    }
                    "COPY" => {
                        handle_uid_copy(&tag, subrest, &current_mailbox, mailboxes, &mut writer)
                            .await?;
                    }
                    "STORE" => {
                        handle_uid_store(&tag, subrest, &current_mailbox, mailboxes, &mut writer)
                            .await?;
                    }
                    _ => {
                        writer
                            .write_all(format!("{tag} BAD unknown UID subcommand\r\n").as_bytes())
                            .await?;
                    }
                }
            }
            "CAPABILITY" => {
                writer.write_all(b"* CAPABILITY IMAP4rev1\r\n").await?;
                writer
                    .write_all(format!("{tag} OK CAPABILITY completed\r\n").as_bytes())
                    .await?;
            }
            "LOGOUT" => {
                writer.write_all(b"* BYE logging out\r\n").await?;
                writer
                    .write_all(format!("{tag} OK LOGOUT completed\r\n").as_bytes())
                    .await?;
                break;
            }
            _ => {
                writer
                    .write_all(format!("{tag} BAD unrecognized command\r\n").as_bytes())
                    .await?;
            }
        }
    }
    Ok(())
}

/// `UID SEARCH <criteria>`. Two shapes are understood: `HEADER Message-ID
/// "<id>"` (milestone 6's Proton dedupe path — matches against the
/// currently-selected mailbox's messages) and everything else, which keeps
/// milestone 0's behavior of matching every seeded message (the only
/// criterion the real client's plain `ALL`-style search ever sent before
/// this milestone).
async fn handle_uid_search(
    tag: &str,
    criteria: &str,
    current_mailbox: &Option<String>,
    mailboxes: &Mailboxes,
    writer: &mut (impl AsyncWriteExt + Unpin),
) -> std::io::Result<()> {
    let guard = mailboxes.lock().expect("mailboxes mutex poisoned");
    let mb = current_mailbox.as_ref().and_then(|n| guard.get(n));

    let upper = criteria.to_ascii_uppercase();
    let uids: Vec<u32> = if upper.starts_with("HEADER MESSAGE-ID") {
        let wanted = criteria
            .splitn(2, '"')
            .nth(1)
            .map(|s| s.trim_end_matches('"'))
            .unwrap_or("");
        mb.map(|mb| {
            mb.messages
                .iter()
                .filter(|m| m.message_id().as_deref() == Some(wanted))
                .map(|m| m.uid)
                .collect()
        })
        .unwrap_or_default()
    } else {
        mb.map(|mb| mb.messages.iter().map(|m| m.uid).collect())
            .unwrap_or_default()
    };
    drop(guard);

    let body = uids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    writer
        .write_all(format!("* SEARCH {body}\r\n").as_bytes())
        .await?;
    writer
        .write_all(format!("{tag} OK UID SEARCH completed\r\n").as_bytes())
        .await?;
    Ok(())
}

async fn handle_uid_fetch(
    tag: &str,
    subrest: &str,
    current_mailbox: &Option<String>,
    mailboxes: &Mailboxes,
    seen: &Arc<Mutex<HashSet<u32>>>,
    writer: &mut (impl AsyncWriteExt + Unpin),
) -> std::io::Result<()> {
    // subrest is "<uid-set> (<data items>)", e.g.
    // "12,34 (FLAGS RFC822)" or "12 (FLAGS BODY.PEEK[HEADER])".
    let mut uid_and_items = subrest.splitn(2, ' ');
    let uidset = uid_and_items.next().unwrap_or("");
    let items_spec = uid_and_items
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')');
    let items_upper = items_spec.to_ascii_uppercase();

    let wants_header_only = items_upper.contains("BODY.PEEK[HEADER")
        || items_upper.contains("BODY[HEADER")
        || items_upper.contains("RFC822.HEADER");
    let is_peek = items_upper.contains("PEEK");
    let response_attr = if wants_header_only {
        "BODY[HEADER]"
    } else if items_upper.contains("RFC822") {
        "RFC822"
    } else {
        // BODY[] / BODY.PEEK[] (full message, no section).
        "BODY[]"
    };

    let wanted: HashSet<u32> = uidset.split(',').filter_map(|s| s.parse().ok()).collect();

    let messages: Vec<FixtureMessage> = {
        let guard = mailboxes.lock().expect("mailboxes mutex poisoned");
        current_mailbox
            .as_ref()
            .and_then(|n| guard.get(n))
            .map(|mb| mb.messages.clone())
            .unwrap_or_default()
    };

    for (idx, m) in messages.iter().enumerate() {
        if !wanted.contains(&m.uid) {
            continue;
        }
        let seq = idx + 1;

        let payload: &[u8] = if wants_header_only {
            // Header section ends at the blank line separating it from the
            // body (RFC 5322), trailing CRLFCRLF included.
            match m.raw.find("\r\n\r\n") {
                Some(pos) => m.raw.as_bytes()[..pos + 4].as_ref(),
                None => m.raw.as_bytes(),
            }
        } else {
            m.raw.as_bytes()
        };

        // `PEEK` suppresses the implicit \Seen side effect (RFC 3501
        // §6.4.5); anything else sets it, persisted across connections so
        // a later fetch/check sees the real accumulated state.
        let now_seen = if is_peek {
            seen.lock().expect("seen mutex poisoned").contains(&m.uid)
        } else {
            seen.lock().expect("seen mutex poisoned").insert(m.uid);
            true
        };
        let flags = if now_seen { "\\Seen" } else { "" };

        writer
            .write_all(
                format!(
                    "* {seq} FETCH (UID {} FLAGS ({flags}) {response_attr} {{{}}}\r\n",
                    m.uid,
                    payload.len()
                )
                .as_bytes(),
            )
            .await?;
        writer.write_all(payload).await?;
        writer.write_all(b")\r\n").await?;
    }
    writer
        .write_all(format!("{tag} OK UID FETCH completed\r\n").as_bytes())
        .await?;
    Ok(())
}

/// `UID COPY <uid> <mailbox>` — the mechanism behind Proton's non-destructive
/// label-as-mailbox apply and Outlook's folder redirect. The target mailbox
/// must already exist (real servers reject `COPY` to a nonexistent mailbox
/// with `NO [TRYCREATE]`; emailibrium's adapters always `CREATE` first).
async fn handle_uid_copy(
    tag: &str,
    subrest: &str,
    current_mailbox: &Option<String>,
    mailboxes: &Mailboxes,
    writer: &mut (impl AsyncWriteExt + Unpin),
) -> std::io::Result<()> {
    let mut parts = subrest.splitn(2, ' ');
    let uid: u32 = parts.next().unwrap_or("").parse().unwrap_or(0);
    let target = parts.next().unwrap_or("").trim().trim_matches('"');

    let mut guard = mailboxes.lock().expect("mailboxes mutex poisoned");
    let Some(source_name) = current_mailbox.clone() else {
        drop(guard);
        writer
            .write_all(format!("{tag} NO no mailbox selected\r\n").as_bytes())
            .await?;
        return Ok(());
    };

    let Some(found) = guard
        .get(&source_name)
        .and_then(|mb| mb.messages.iter().find(|m| m.uid == uid).cloned())
    else {
        drop(guard);
        writer
            .write_all(format!("{tag} NO [NONEXISTENT] UID {uid} not found\r\n").as_bytes())
            .await?;
        return Ok(());
    };

    let Some(target_mb) = guard.get_mut(target) else {
        drop(guard);
        writer
            .write_all(format!("{tag} NO [TRYCREATE] mailbox '{target}' does not exist\r\n").as_bytes())
            .await?;
        return Ok(());
    };

    let new_uid = target_mb.next_uid;
    target_mb.next_uid += 1;
    target_mb.messages.push(FixtureMessage {
        uid: new_uid,
        raw: found.raw,
        deleted: false,
    });
    drop(guard);

    writer
        .write_all(format!("{tag} OK [COPYUID 1 {uid} {new_uid}] COPY completed\r\n").as_bytes())
        .await?;
    Ok(())
}

/// `UID STORE <uid> +FLAGS (\Deleted)` / `-FLAGS (...)`. Only `\Deleted` is
/// tracked (the only flag this fixture's adapters under test actually set
/// via STORE) — any other flag name is accepted and silently ignored,
/// matching the "never reject something we don't model" ethos of the rest
/// of this fixture.
async fn handle_uid_store(
    tag: &str,
    subrest: &str,
    current_mailbox: &Option<String>,
    mailboxes: &Mailboxes,
    writer: &mut (impl AsyncWriteExt + Unpin),
) -> std::io::Result<()> {
    let mut parts = subrest.splitn(2, ' ');
    let uid: u32 = parts.next().unwrap_or("").parse().unwrap_or(0);
    let rest = parts.next().unwrap_or("");
    let upper = rest.to_ascii_uppercase();
    let sets_deleted = upper.starts_with('+') && upper.contains("\\DELETED");
    let clears_deleted = upper.starts_with('-') && upper.contains("\\DELETED");

    if let Some(name) = current_mailbox {
        let mut guard = mailboxes.lock().expect("mailboxes mutex poisoned");
        if let Some(mb) = guard.get_mut(name) {
            if let Some(m) = mb.messages.iter_mut().find(|m| m.uid == uid) {
                if sets_deleted {
                    m.deleted = true;
                } else if clears_deleted {
                    m.deleted = false;
                }
            }
        }
    }

    writer
        .write_all(format!("{tag} OK UID STORE completed\r\n").as_bytes())
        .await?;
    Ok(())
}
