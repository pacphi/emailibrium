//! Minimal in-process IMAP server for integration tests (mail-agent plan,
//! Milestone 0).
//!
//! This is not a conformant IMAP4rev1 server — it implements only the exact
//! command shapes emailibrium's real `ImapProvider` (built on `async-imap`)
//! sends during a login + select + search + fetch round trip: `LOGIN`,
//! `SELECT`/`EXAMINE`, `UID SEARCH`, `UID FETCH` with `RFC822`,
//! `BODY.PEEK[]`/`BODY[]`, or `BODY.PEEK[HEADER]`/`BODY[HEADER]` data items,
//! `LOGOUT`. It never validates credentials — the point is to exercise the
//! real client code against a real (if tiny) wire-protocol server, not to
//! test auth failure paths (those are already covered by the config-validation
//! unit tests in `src/email/imap.rs`).
//!
//! It tracks real per-UID `\Seen` state across connections: a fetch item
//! without `PEEK` (`RFC822`, `BODY[]`, `BODY[HEADER]`) sets `\Seen` as a side
//! effect, matching real IMAP server behavior (RFC 3501 §6.4.5); a `PEEK`
//! variant never does. This is what proves/disproves "scanning marks nothing
//! read" for the real client code, not a reimplementation of it.
//!
//! CONDSTORE, Gmail `X-GM-EXT-1`, Proton `Labels/` semantics, and
//! STORE/MOVE/COPY are out of scope here — see `docs/plan/mail-agent-2026-10.md`
//! §9 for where those land in later milestones.

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
}

impl FixtureMessage {
    /// Build a minimal, realistic RFC822 message from header fields. Bodies
    /// are intentionally short — this fixture is about protocol conformance,
    /// not volume.
    pub fn new(uid: u32, from: &str, to: &str, subject: &str, date_rfc2822: &str, body: &str) -> Self {
        let raw = format!(
            "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date_rfc2822}\r\nMessage-ID: <fixture-{uid}@fake-imap.test>\r\n\r\n{body}\r\n"
        );
        Self { uid, raw }
    }
}

/// A running fake IMAP server bound to an ephemeral loopback port.
pub struct FakeImapServer {
    pub addr: SocketAddr,
    seen: Arc<Mutex<HashSet<u32>>>,
}

impl FakeImapServer {
    /// Bind `127.0.0.1:0` and start serving the given fixture mailbox to
    /// any number of incoming connections in the background. Returns once
    /// the listener is bound; the serving loop runs on spawned tasks.
    pub async fn spawn(messages: Vec<FixtureMessage>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake IMAP listener");
        let addr = listener.local_addr().expect("fake IMAP local_addr");
        let messages = Arc::new(messages);
        let seen: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));

        {
            let messages = Arc::clone(&messages);
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                loop {
                    let (stream, _peer) = match listener.accept().await {
                        Ok(pair) => pair,
                        Err(_) => break,
                    };
                    let messages = Arc::clone(&messages);
                    let seen = Arc::clone(&seen);
                    tokio::spawn(async move {
                        let _ = serve_connection(stream, &messages, &seen).await;
                    });
                }
            });
        }

        Self { addr, seen }
    }

    /// Whether the fake server's persisted state currently has `\Seen` set
    /// for this UID — the real-side-effect assertion for the `\Seen`
    /// side-effect fix: a `PEEK` fetch must leave this `false`.
    pub fn is_seen(&self, uid: u32) -> bool {
        self.seen.lock().expect("seen mutex poisoned").contains(&uid)
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    messages: &[FixtureMessage],
    seen: &Arc<Mutex<HashSet<u32>>>,
) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    writer.write_all(b"* OK Fake IMAP ready\r\n").await?;

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
            "SELECT" | "EXAMINE" => {
                let exists = messages.len();
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
            "UID" => {
                let mut sub = rest.splitn(2, ' ');
                let subcmd = sub.next().unwrap_or("").to_ascii_uppercase();
                let subrest = sub.next().unwrap_or("");

                match subcmd.as_str() {
                    "SEARCH" => {
                        // Criteria are ignored — the fixture always matches
                        // every seeded message, which is all the real
                        // client's "ALL" search needs for this round trip.
                        let uids: Vec<String> =
                            messages.iter().map(|m| m.uid.to_string()).collect();
                        writer
                            .write_all(format!("* SEARCH {}\r\n", uids.join(" ")).as_bytes())
                            .await?;
                        writer
                            .write_all(format!("{tag} OK UID SEARCH completed\r\n").as_bytes())
                            .await?;
                    }
                    "FETCH" => {
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

                        let wants_header_only = items_upper.contains("BODY.PEEK[HEADER]")
                            || items_upper.contains("BODY[HEADER]")
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

                        let wanted: HashSet<u32> =
                            uidset.split(',').filter_map(|s| s.parse().ok()).collect();

                        for (idx, m) in messages.iter().enumerate() {
                            if !wanted.contains(&m.uid) {
                                continue;
                            }
                            let seq = idx + 1;

                            let payload: &[u8] = if wants_header_only {
                                // Header section ends at the blank line
                                // separating it from the body (RFC 5322),
                                // trailing CRLFCRLF included.
                                match m.raw.find("\r\n\r\n") {
                                    Some(pos) => m.raw.as_bytes()[..pos + 4].as_ref(),
                                    None => m.raw.as_bytes(),
                                }
                            } else {
                                m.raw.as_bytes()
                            };

                            // `PEEK` suppresses the implicit \Seen side
                            // effect (RFC 3501 §6.4.5); anything else sets
                            // it, persisted across connections so a later
                            // fetch/check sees the real accumulated state.
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
