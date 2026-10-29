//! Outlook device-code sign-in flows (milestone 6, beta).
//!
//! Microsoft's device authorization grant (RFC 8628) hands back a
//! `device_code` that anyone can redeem for the user's tokens, so it never
//! leaves the server: the browser gets a random flow id plus the `user_code`
//! to type at `verification_uri`, and polls by flow id. Each poll is throttled
//! to Microsoft's `interval` (raised by 5 seconds on `slow_down`, RFC 8628
//! §3.5) so a fast client can't earn penalties. On success the access token is
//! verified with a real XOAUTH2 IMAP login before the account is saved — this
//! also rejects signing in at the verification page as a different mailbox.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use uuid::Uuid;

use super::imap::{ImapAuth, ImapConfig, ImapEncryption, ImapProvider, ProviderProfile};
use super::oauth::{DeviceCodePoll, OAuthError, OAuthManager};
use super::provider::EmailProvider;
use crate::vectors::config::OutlookDeviceCodeConfig;

/// RFC 8628 §3.5: on `slow_down`, wait this much longer between polls.
const SLOW_DOWN_STEP_SECS: u64 = 5;

/// What the browser needs to show the user. Deliberately has no
/// `device_code`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFlowStarted {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Result of one poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceFlowStatus {
    /// Not signed in yet; poll again after `interval` seconds.
    Pending { interval: u64 },
    /// Microsoft asked for slower polling; poll again after `interval`.
    SlowDown { interval: u64 },
    /// The code expired before sign-in finished. Start again.
    Expired,
    /// The user declined consent.
    Declined,
    /// Signed in, verified over IMAP, and saved.
    Connected { account_id: String, email: String },
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceFlowError {
    #[error("no device-code sign-in with that id (it may have finished or expired)")]
    UnknownFlow,
    #[error("Outlook device-code sign-in is not configured: missing env var {0}")]
    NotConfigured(String),
    #[error("Microsoft sign-in failed: {0}")]
    Microsoft(#[from] OAuthError),
    #[error("signed in, but the IMAP login as {email} failed: {reason}")]
    ImapLogin { email: String, reason: String },
}

struct PendingFlow {
    device_code: String,
    email: String,
    client_id: String,
    interval: u64,
    next_poll_at: Instant,
    expires_at: Instant,
}

/// In-memory registry of sign-ins in progress. Held on `AppState`.
#[derive(Default)]
pub struct DeviceFlowRegistry {
    flows: Mutex<HashMap<String, PendingFlow>>,
}

impl DeviceFlowRegistry {
    /// Ask Microsoft for a device code for `email` and remember it under a
    /// new flow id.
    pub async fn start(
        &self,
        oauth: &OAuthManager,
        config: &OutlookDeviceCodeConfig,
        email: &str,
    ) -> Result<DeviceFlowStarted, DeviceFlowError> {
        let client_id = std::env::var(&config.client_id_env)
            .map_err(|_| DeviceFlowError::NotConfigured(config.client_id_env.clone()))?;
        let started = oauth.start_outlook_device_code(config, &client_id).await?;

        let now = Instant::now();
        let flow_id = Uuid::new_v4().to_string();
        {
            let mut flows = self.flows.lock().expect("device flow lock poisoned");
            flows.retain(|_, f| f.expires_at > now);
            flows.insert(
                flow_id.clone(),
                PendingFlow {
                    device_code: started.device_code,
                    email: email.to_string(),
                    client_id,
                    interval: started.interval,
                    next_poll_at: now + Duration::from_secs(started.interval),
                    expires_at: now + Duration::from_secs(started.expires_in),
                },
            );
        }

        Ok(DeviceFlowStarted {
            flow_id,
            user_code: started.user_code,
            verification_uri: started.verification_uri,
            expires_in: started.expires_in,
            interval: started.interval,
        })
    }

    /// Poll Microsoft once for `flow_id` (or not at all, if called before the
    /// flow's interval has passed). On success, verify the token with an
    /// XOAUTH2 IMAP login and save the account.
    pub async fn poll(
        &self,
        oauth: &OAuthManager,
        config: &OutlookDeviceCodeConfig,
        flow_id: &str,
    ) -> Result<DeviceFlowStatus, DeviceFlowError> {
        let now = Instant::now();
        // Take what the network call needs and release the lock before
        // awaiting: a std MutexGuard must not be held across `.await`.
        let (device_code, client_id, email) = {
            let mut flows = self.flows.lock().expect("device flow lock poisoned");
            let flow = flows.get(flow_id).ok_or(DeviceFlowError::UnknownFlow)?;
            if flow.expires_at <= now {
                flows.remove(flow_id);
                return Ok(DeviceFlowStatus::Expired);
            }
            if now < flow.next_poll_at {
                return Ok(DeviceFlowStatus::Pending {
                    interval: flow.interval,
                });
            }
            (
                flow.device_code.clone(),
                flow.client_id.clone(),
                flow.email.clone(),
            )
        };

        let result = oauth
            .poll_outlook_device_code(config, &client_id, &device_code)
            .await?;

        let tokens = match result {
            DeviceCodePoll::Pending => {
                return Ok(DeviceFlowStatus::Pending {
                    interval: self.schedule_next(flow_id, 0),
                })
            }
            DeviceCodePoll::SlowDown => {
                return Ok(DeviceFlowStatus::SlowDown {
                    interval: self.schedule_next(flow_id, SLOW_DOWN_STEP_SECS),
                })
            }
            DeviceCodePoll::Expired => {
                self.remove(flow_id);
                return Ok(DeviceFlowStatus::Expired);
            }
            DeviceCodePoll::Declined => {
                self.remove(flow_id);
                return Ok(DeviceFlowStatus::Declined);
            }
            DeviceCodePoll::Done(tokens) => tokens,
        };
        // The device code is spent either way; a failed IMAP check means
        // starting over, not re-polling.
        self.remove(flow_id);

        // Host and port come from server config, not the request, so the
        // SSRF guard used for user-supplied IMAP hosts isn't needed here.
        let imap = ImapProvider::new(ImapConfig {
            host: config.imap_host.clone(),
            port: config.imap_port,
            encryption: ImapEncryption::from_request_str(&config.imap_encryption),
            username: email.clone(),
            auth: ImapAuth::OAuthBearer(tokens.access_token.clone()),
            profile: ProviderProfile::OutlookImap,
            mailbox: "INBOX".to_string(),
            archive_folder: "Archive".to_string(),
            smtp_host: None,
            smtp_port: 587,
            pinned_addr: None,
        });
        imap.authenticate("")
            .await
            .map_err(|e| DeviceFlowError::ImapLogin {
                email: email.clone(),
                reason: e.to_string(),
            })?;

        let account_id = oauth
            .find_account_id_by_email(&email)
            .await?
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        oauth
            .save_imap_oauth_account(
                &account_id,
                &email,
                &tokens,
                &config.imap_host,
                config.imap_port,
                ImapEncryption::from_request_str(&config.imap_encryption),
                ProviderProfile::OutlookImap,
            )
            .await?;

        Ok(DeviceFlowStatus::Connected { account_id, email })
    }

    /// Push the flow's next allowed poll `interval + extra` seconds out and
    /// return the (possibly raised) interval.
    fn schedule_next(&self, flow_id: &str, extra: u64) -> u64 {
        let mut flows = self.flows.lock().expect("device flow lock poisoned");
        match flows.get_mut(flow_id) {
            Some(flow) => {
                flow.interval += extra;
                flow.next_poll_at = Instant::now() + Duration::from_secs(flow.interval);
                flow.interval
            }
            None => extra,
        }
    }

    fn remove(&self, flow_id: &str) {
        self.flows
            .lock()
            .expect("device flow lock poisoned")
            .remove(flow_id);
    }
}
