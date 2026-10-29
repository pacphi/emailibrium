//! Opt-in live half of the provider-adapter conformance test (mail-agent
//! plan, milestone 6): the same label → undo and archive → move-back round
//! trips as `provider_conformance.rs`, against one real IMAP mailbox.
//!
//! The live test is `#[ignore]`d and runs only when named explicitly, with
//! every `EMAILIBRIUM_TEST_IMAP_*` setting present — a missing setting is an
//! error, never a skip-pass — and `EMAILIBRIUM_TEST_IMAP_ALLOW_WRITES=yes`,
//! because it writes to the mailbox. Use a dedicated test account.
//!
//! What it writes, and where:
//! - It creates two mailboxes with a random suffix
//!   (`Emailibrium-Conformance-<id>`, `Emailibrium-Conformance-Archive-<id>`)
//!   and refuses to continue unless the first is empty.
//! - It appends one synthetic message (`Message-ID:
//!   <emailibrium-conformance-<id>@test.invalid>`) there and works only on
//!   that message, with labels `Kind/EmailibriumTest<id>` and
//!   `State/EmailibriumTest<id>`. INBOX is never written; the run fails if
//!   the synthetic message ever shows up in INBOX.
//! - Cleanup always runs: it deletes exactly the mailboxes it created (never
//!   a parent such as `Kind`), first copying the synthetic message to the
//!   Trash mailbox on Gmail, where deleting a label mailbox would otherwise
//!   leave the message in All Mail. Anything it could not remove is printed.
//!
//! Output names the move strategy and label path the server actually used.
//! Credentials and raw server text are never printed.

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use async_imap::types::{Flag, NameAttribute};
use futures::TryStreamExt;

use emailibrium::email::imap::{
    ImapAuth, ImapConfig, ImapEncryption, ImapProvider, ImapSession, ProviderProfile,
};
use emailibrium::email::provider::{EmailProvider, MoveKind};

use fake_imap::FakeImapServer;

const LIVE_TIMEOUT: Duration = Duration::from_secs(180);

const KEYS: [&str; 6] = [
    "EMAILIBRIUM_TEST_IMAP_HOST",
    "EMAILIBRIUM_TEST_IMAP_PORT",
    "EMAILIBRIUM_TEST_IMAP_ENCRYPTION",
    "EMAILIBRIUM_TEST_IMAP_USERNAME",
    "EMAILIBRIUM_TEST_IMAP_PROFILE",
    "EMAILIBRIUM_TEST_IMAP_ALLOW_WRITES",
];
const PASSWORD: &str = "EMAILIBRIUM_TEST_IMAP_PASSWORD";
const OAUTH_TOKEN: &str = "EMAILIBRIUM_TEST_IMAP_OAUTH_TOKEN";

// Deliberately no Debug implementation: credentials must never enter output.
struct Settings {
    host: String,
    port: u16,
    encryption: ImapEncryption,
    username: String,
    auth: ImapAuth,
    profile: ProviderProfile,
}

#[derive(Debug, thiserror::Error)]
enum LiveError {
    #[error("required test setting is missing: {0}")]
    MissingConfiguration(&'static str),
    #[error("test setting is invalid: {0}")]
    InvalidSetting(&'static str),
    #[error(
        "EMAILIBRIUM_TEST_IMAP_ALLOW_WRITES must be exactly `yes`: this test writes to the mailbox"
    )]
    WritesNotAllowed,
    #[error("set exactly one of {PASSWORD} or {OAUTH_TOKEN}")]
    AmbiguousCredential,
    #[error("{0} failed")]
    Stage(&'static str),
    #[error("scratch mailbox already had messages; refusing to write")]
    ScratchNotEmpty,
    #[error("mailbox state after {0} undo differs from before")]
    RoundTripMismatch(&'static str),
    #[error("the synthetic message appeared in INBOX")]
    InboxLeak,
    #[error("live run timed out")]
    Timeout,
}

fn settings(get: impl Fn(&str) -> Option<String>) -> Result<Settings, LiveError> {
    let required = |key: &'static str| {
        get(key)
            .filter(|v| !v.trim().is_empty())
            .ok_or(LiveError::MissingConfiguration(key))
    };
    for key in KEYS {
        required(key)?;
    }
    if required("EMAILIBRIUM_TEST_IMAP_ALLOW_WRITES")? != "yes" {
        return Err(LiveError::WritesNotAllowed);
    }
    let auth = match (
        get(PASSWORD).filter(|v| !v.is_empty()),
        get(OAUTH_TOKEN).filter(|v| !v.is_empty()),
    ) {
        (Some(p), None) => ImapAuth::Password(p),
        (None, Some(t)) => ImapAuth::OAuthBearer(t),
        (None, None) => return Err(LiveError::MissingConfiguration(PASSWORD)),
        (Some(_), Some(_)) => return Err(LiveError::AmbiguousCredential),
    };
    let profile = match required("EMAILIBRIUM_TEST_IMAP_PROFILE")?.as_str() {
        "generic" => ProviderProfile::Generic,
        "proton_bridge" => ProviderProfile::ProtonBridge,
        "outlook_imap" => ProviderProfile::OutlookImap,
        _ => return Err(LiveError::InvalidSetting("EMAILIBRIUM_TEST_IMAP_PROFILE")),
    };
    let encryption = match required("EMAILIBRIUM_TEST_IMAP_ENCRYPTION")?.as_str() {
        "ssl" => ImapEncryption::Ssl,
        "tls" => ImapEncryption::StartTls,
        "none" => ImapEncryption::Plaintext,
        _ => {
            return Err(LiveError::InvalidSetting(
                "EMAILIBRIUM_TEST_IMAP_ENCRYPTION",
            ))
        }
    };
    let port = required("EMAILIBRIUM_TEST_IMAP_PORT")?
        .parse()
        .map_err(|_| LiveError::InvalidSetting("EMAILIBRIUM_TEST_IMAP_PORT"))?;
    Ok(Settings {
        host: required("EMAILIBRIUM_TEST_IMAP_HOST")?,
        port,
        encryption,
        username: required("EMAILIBRIUM_TEST_IMAP_USERNAME")?,
        auth,
        profile,
    })
}

/// What the run did, for the person reading the output. No secrets.
#[derive(Debug, Default)]
struct Report {
    move_strategy: &'static str,
    keywords_kept: bool,
    label_path: &'static str,
    gmail: bool,
    created: Vec<String>,
    left_behind: Vec<String>,
    new_parents: Vec<String>,
}

struct Names {
    message_id: String,
    scratch: String,
    archive: String,
    kind: String,
    state: String,
}

impl Names {
    fn new() -> Self {
        let id = uuid::Uuid::new_v4().simple().to_string();
        Self {
            message_id: format!("<emailibrium-conformance-{id}@test.invalid>"),
            scratch: format!("Emailibrium-Conformance-{id}"),
            archive: format!("Emailibrium-Conformance-Archive-{id}"),
            kind: format!("Kind/EmailibriumTest{id}"),
            state: format!("State/EmailibriumTest{id}"),
        }
    }

    /// Every mailbox this run can create, deepest first.
    fn tracked(&self) -> Vec<String> {
        vec![
            format!("Labels/{}", self.kind),
            format!("Labels/{}", self.state),
            self.kind.clone(),
            self.archive.clone(),
            self.scratch.clone(),
        ]
    }
}

fn config(s: &Settings, mailbox: &str, archive: &str) -> ImapConfig {
    ImapConfig {
        host: s.host.clone(),
        port: s.port,
        encryption: s.encryption,
        username: s.username.clone(),
        auth: s.auth.clone(),
        profile: s.profile,
        mailbox: mailbox.to_string(),
        archive_folder: archive.to_string(),
        smtp_host: None,
        smtp_port: 587,
        pinned_addr: None,
    }
}

async fn session(s: &Settings, n: &Names) -> Result<ImapSession, LiveError> {
    ImapProvider::new(config(s, &n.scratch, &n.archive))
        .open_session_for_tests()
        .await
        .map_err(|_| LiveError::Stage("login"))
}

/// UIDs of the synthetic message in `mailbox` (empty if it doesn't exist).
async fn find(sess: &mut ImapSession, mailbox: &str, message_id: &str) -> Vec<u32> {
    if sess.examine(mailbox).await.is_err() {
        return Vec::new();
    }
    let mut uids: Vec<u32> = sess
        .uid_search(format!("HEADER Message-ID \"{message_id}\""))
        .await
        .map(|set| set.into_iter().collect())
        .unwrap_or_default();
    uids.sort_unstable();
    uids
}

fn one(uids: Vec<u32>, stage: &'static str) -> Result<u32, LiveError> {
    match uids.as_slice() {
        [uid] => Ok(*uid),
        _ => Err(LiveError::Stage(stage)),
    }
}

/// Per tracked mailbox: the custom keywords on each copy of the synthetic
/// message. UIDs and empty mailboxes are left out (see provider_conformance).
async fn snapshot(sess: &mut ImapSession, n: &Names) -> BTreeMap<String, Vec<Vec<String>>> {
    let mut out = BTreeMap::new();
    for mailbox in n.tracked() {
        let uids = find(sess, &mailbox, &n.message_id).await;
        if uids.is_empty() {
            continue;
        }
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut copies: Vec<Vec<String>> = match sess.uid_fetch(set, "FLAGS").await {
            Ok(stream) => stream
                .try_collect::<Vec<_>>()
                .await
                .unwrap_or_default()
                .iter()
                .map(|f| {
                    let mut kws: Vec<String> = f
                        .flags()
                        .filter_map(|flag| match flag {
                            Flag::Custom(c) => Some(c.to_string()),
                            _ => None,
                        })
                        .collect();
                    kws.sort();
                    kws
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        copies.sort();
        out.insert(mailbox, copies);
    }
    out
}

/// `Some(exists)` from LIST, or `None` when LIST itself failed (unknown).
async fn mailbox_exists(sess: &mut ImapSession, name: &str) -> Option<bool> {
    let stream = sess.list(Some(""), Some(name)).await.ok()?;
    let names = stream.try_collect::<Vec<_>>().await.ok()?;
    Some(names.iter().any(|m| m.name() == name))
}

async fn trash_mailbox(sess: &mut ImapSession) -> Option<String> {
    let names = sess
        .list(Some(""), Some("*"))
        .await
        .ok()?
        .try_collect::<Vec<_>>()
        .await
        .ok()?;
    names
        .iter()
        .find(|m| {
            m.attributes()
                .iter()
                .any(|a| matches!(a, NameAttribute::Trash))
        })
        .map(|m| m.name().to_string())
}

/// Delete exactly the mailboxes this run created. On Gmail, first copy the
/// synthetic message to Trash so it doesn't stay in All Mail.
async fn cleanup(sess: &mut ImapSession, n: &Names, report: &mut Report) {
    let trash = if report.gmail {
        trash_mailbox(sess).await
    } else {
        None
    };
    for mailbox in n.tracked() {
        // If LIST can't say, still try to delete what this run created.
        let created = report.created.contains(&mailbox);
        match mailbox_exists(sess, &mailbox).await {
            Some(false) => continue,
            None if !created => continue,
            _ => {}
        }
        if let Some(trash) = &trash {
            let uids = find(sess, &mailbox, &n.message_id).await;
            if !uids.is_empty() && sess.select(&mailbox).await.is_ok() {
                let set = uids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = sess.uid_copy(set, trash).await;
            }
            // Leave the mailbox before DELETE without CLOSE (which expunges):
            // a read-only EXAMINE of INBOX changes nothing.
            let _ = sess.examine("INBOX").await;
        }
        if sess.delete(&mailbox).await.is_err() {
            report.left_behind.push(mailbox);
        }
    }
}

async fn round_trip(
    s: &Settings,
    n: &Names,
    sess: &mut ImapSession,
    report: &mut Report,
) -> Result<(), LiveError> {
    let before = snapshot(sess, n).await;
    let scratch = ImapProvider::new(config(s, &n.scratch, &n.archive));
    let uid = one(find(sess, &n.scratch, &n.message_id).await, "locate")?;
    let labels = vec![n.kind.clone(), n.state.clone()];

    scratch
        .label_message("unused", &uid.to_string(), &labels)
        .await
        .map_err(|_| LiveError::Stage("label"))?;

    let still_in_scratch = !find(sess, &n.scratch, &n.message_id).await.is_empty();
    report.label_path = match (s.profile, still_in_scratch) {
        (ProviderProfile::ProtonBridge, _) => "Labels/ copies",
        (_, true) => "keywords",
        (_, false) => "folder",
    };

    if still_in_scratch {
        scratch
            .remove_labels("unused", &uid.to_string(), &labels)
            .await
            .map_err(|_| LiveError::Stage("unlabel"))?;
    } else {
        scratch
            .remove_labels("unused", &uid.to_string(), std::slice::from_ref(&n.state))
            .await
            .map_err(|_| LiveError::Stage("unlabel state"))?;
        let moved = one(
            find(sess, &n.kind, &n.message_id).await,
            "locate in label folder",
        )?;
        ImapProvider::new(config(s, &n.kind, &n.archive))
            .move_message("unused", &moved.to_string(), &n.scratch, MoveKind::Folder)
            .await
            .map_err(|_| LiveError::Stage("move back from label folder"))?;
    }
    if snapshot(sess, n).await != before {
        return Err(LiveError::RoundTripMismatch("label"));
    }

    // Archive, then move back into the scratch mailbox. Not
    // unarchive_message: that always moves to the real INBOX.
    let uid = one(find(sess, &n.scratch, &n.message_id).await, "locate")?;
    scratch
        .archive_message("unused", &uid.to_string())
        .await
        .map_err(|_| LiveError::Stage("archive"))?;
    let archived = one(
        find(sess, &n.archive, &n.message_id).await,
        "locate archived",
    )?;
    ImapProvider::new(config(s, &n.archive, &n.archive))
        .move_message(
            "unused",
            &archived.to_string(),
            &n.scratch,
            MoveKind::Folder,
        )
        .await
        .map_err(|_| LiveError::Stage("move back from archive"))?;
    if snapshot(sess, n).await != before {
        return Err(LiveError::RoundTripMismatch("archive"));
    }

    if !find(sess, "INBOX", &n.message_id).await.is_empty() {
        return Err(LiveError::InboxLeak);
    }
    Ok(())
}

async fn run_round_trip(s: &Settings) -> Result<Report, LiveError> {
    let n = Names::new();
    let mut report = Report::default();
    let mut sess = session(s, &n).await?;

    let caps = sess
        .capabilities()
        .await
        .map_err(|_| LiveError::Stage("capability"))?;
    report.move_strategy = if caps.has_str("MOVE") {
        "MOVE"
    } else if caps.has_str("UIDPLUS") {
        "UIDPLUS"
    } else {
        "guarded plain EXPUNGE"
    };
    report.gmail = caps.has_str("X-GM-EXT-1");
    let parents = ["Kind", "State", "Labels"];
    let mut parents_before = HashMap::new();
    for p in parents {
        parents_before.insert(p, mailbox_exists(&mut sess, p).await.unwrap_or(true));
    }

    let result = async {
        for name in [&n.scratch, &n.archive] {
            sess.create(name)
                .await
                .map_err(|_| LiveError::Stage("create scratch mailbox"))?;
            report.created.push(name.clone());
        }
        let scratch = sess
            .examine(&n.scratch)
            .await
            .map_err(|_| LiveError::Stage("examine scratch"))?;
        if scratch.exists != 0 {
            return Err(LiveError::ScratchNotEmpty);
        }
        report.keywords_kept = scratch.permanent_flags.is_empty()
            || scratch
                .permanent_flags
                .iter()
                .any(|f| matches!(f, Flag::MayCreate));
        let message = format!(
            "From: Emailibrium Conformance <conformance@test.invalid>\r\n\
             To: {}\r\n\
             Subject: emailibrium conformance test (safe to delete)\r\n\
             Message-ID: {}\r\n\
             Date: Thu, 29 Oct 2026 12:00:00 +0000\r\n\r\n\
             Synthetic message for a live provider conformance run.\r\n",
            s.username, n.message_id
        );
        sess.append(&n.scratch, None, None, message.as_bytes())
            .await
            .map_err(|_| LiveError::Stage("append"))?;
        round_trip(s, &n, &mut sess, &mut report).await
    }
    .await;

    cleanup(&mut sess, &n, &mut report).await;
    for p in parents {
        if !parents_before[p] && mailbox_exists(&mut sess, p).await == Some(true) {
            report.new_parents.push(p.to_string());
        }
    }
    let _ = sess.logout().await;
    result.map(|()| report)
}

#[tokio::test]
#[ignore = "explicit dedicated test-account settings required; WRITES to the mailbox (scratch folders only)"]
async fn live_imap_round_trip() -> Result<(), LiveError> {
    let s = settings(|key| std::env::var(key).ok())?;
    let report = tokio::time::timeout(LIVE_TIMEOUT, run_round_trip(&s))
        .await
        .map_err(|_| LiveError::Timeout)??;
    eprintln!(
        "live conformance OK: move strategy = {}, keywords kept = {}, label path = {}, gmail = {}",
        report.move_strategy, report.keywords_kept, report.label_path, report.gmail
    );
    eprintln!("created and removed: {:?}", report.created);
    if !report.left_behind.is_empty() {
        eprintln!(
            "could NOT remove (delete by hand): {:?}",
            report.left_behind
        );
    }
    if !report.new_parents.is_empty() {
        eprintln!(
            "the server created empty parent folder(s) {:?}; left in place",
            report.new_parents
        );
    }
    assert!(report.left_behind.is_empty(), "cleanup incomplete");
    Ok(())
}

// --- The harness itself, proven against the fake IMAP server -------------

fn fake_settings(server: &FakeImapServer, profile: ProviderProfile) -> Settings {
    Settings {
        host: "127.0.0.1".to_string(),
        port: server.addr.port(),
        encryption: ImapEncryption::Plaintext,
        username: "conformance@example.test".to_string(),
        auth: ImapAuth::Password("fixture-app-password".to_string()),
        profile,
    }
}

async fn fake(capabilities: &[&str], permanent_flags: Option<&str>) -> FakeImapServer {
    let mut seed = HashMap::new();
    seed.insert("INBOX".to_string(), Vec::new());
    let server = FakeImapServer::spawn_with_mailboxes_and_capabilities(seed, capabilities).await;
    if let Some(flags) = permanent_flags {
        server.set_permanent_flags(flags);
    }
    server
}

fn assert_cleaned(server: &FakeImapServer, report: &Report) {
    assert!(report.left_behind.is_empty(), "{:?}", report.left_behind);
    for name in &report.created {
        assert!(!server.mailbox_exists(name), "{name} not removed");
    }
    assert!(server.mailbox_uids("INBOX").is_empty(), "INBOX untouched");
}

#[tokio::test]
async fn harness_round_trips_generic_keywords_on_the_fake() {
    let server = fake(&["MOVE", "UIDPLUS"], Some("\\Seen \\Deleted \\*")).await;
    let report = run_round_trip(&fake_settings(&server, ProviderProfile::Generic))
        .await
        .expect("round trip");
    assert_eq!(report.label_path, "keywords");
    assert_eq!(report.move_strategy, "MOVE");
    assert!(report.keywords_kept);
    assert_cleaned(&server, &report);
}

#[tokio::test]
async fn harness_round_trips_generic_folder_fallback_on_the_fake() {
    // No MOVE/UIDPLUS: archive takes the guarded plain-EXPUNGE path.
    let server = fake(&[], Some("\\Seen \\Deleted")).await;
    let report = run_round_trip(&fake_settings(&server, ProviderProfile::Generic))
        .await
        .expect("round trip");
    assert_eq!(report.label_path, "folder");
    assert_eq!(report.move_strategy, "guarded plain EXPUNGE");
    assert!(!report.keywords_kept);
    assert_cleaned(&server, &report);
}

#[tokio::test]
async fn harness_round_trips_proton_on_the_fake() {
    let server = fake(&["MOVE", "UIDPLUS"], None).await;
    let report = run_round_trip(&fake_settings(&server, ProviderProfile::ProtonBridge))
        .await
        .expect("round trip");
    assert_eq!(report.label_path, "Labels/ copies");
    assert_cleaned(&server, &report);
}

#[tokio::test]
async fn harness_round_trips_outlook_on_the_fake() {
    let server = fake(&["UIDPLUS"], None).await;
    let report = run_round_trip(&fake_settings(&server, ProviderProfile::OutlookImap))
        .await
        .expect("round trip");
    assert_eq!(report.label_path, "folder");
    assert_eq!(report.move_strategy, "UIDPLUS");
    assert_cleaned(&server, &report);
}

#[test]
fn every_setting_is_required_and_writes_need_explicit_yes() {
    let full = |overrides: &[(&str, Option<&str>)]| {
        let mut map: HashMap<&str, Option<&str>> = HashMap::from([
            ("EMAILIBRIUM_TEST_IMAP_HOST", Some("imap.example.test")),
            ("EMAILIBRIUM_TEST_IMAP_PORT", Some("993")),
            ("EMAILIBRIUM_TEST_IMAP_ENCRYPTION", Some("ssl")),
            ("EMAILIBRIUM_TEST_IMAP_USERNAME", Some("t@example.test")),
            ("EMAILIBRIUM_TEST_IMAP_PROFILE", Some("generic")),
            ("EMAILIBRIUM_TEST_IMAP_ALLOW_WRITES", Some("yes")),
            (PASSWORD, Some("app-password")),
            (OAUTH_TOKEN, None),
        ]);
        for (k, v) in overrides {
            map.insert(k, *v);
        }
        settings(|key| map.get(key).copied().flatten().map(str::to_string))
    };

    assert!(full(&[]).is_ok());
    for key in KEYS {
        assert!(
            matches!(full(&[(key, None)]), Err(LiveError::MissingConfiguration(k)) if k == key),
            "{key}"
        );
    }
    assert!(matches!(
        full(&[("EMAILIBRIUM_TEST_IMAP_ALLOW_WRITES", Some("true"))]),
        Err(LiveError::WritesNotAllowed)
    ));
    assert!(matches!(
        full(&[(PASSWORD, None)]),
        Err(LiveError::MissingConfiguration(_))
    ));
    assert!(matches!(
        full(&[(OAUTH_TOKEN, Some("token"))]),
        Err(LiveError::AmbiguousCredential)
    ));
    assert!(matches!(
        full(&[("EMAILIBRIUM_TEST_IMAP_PROFILE", Some("gmail"))]),
        Err(LiveError::InvalidSetting(_))
    ));
}
