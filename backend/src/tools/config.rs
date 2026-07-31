//! `config/tools.yaml` — per-tool exposure, confirmation and rate-limit policy.
//!
//! Policy lives in configuration rather than in code so the MCP server and the
//! chat orchestrator cannot disagree about which tools exist or which need
//! confirmation. Tools missing from the file take the defaults below.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Fallbacks applied to any tool without an explicit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefaults {
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_minute: u32,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_rate_limit() -> u32 {
    20
}

fn default_timeout_ms() -> u64 {
    10_000
}

impl Default for ToolDefaults {
    fn default() -> Self {
        Self {
            rate_limit_per_minute: default_rate_limit(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

/// One tool's entry as written in the YAML file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolEntry {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub requires_confirmation: Option<bool>,
    #[serde(default)]
    pub rate_limit_per_minute: Option<u32>,
    /// Marks a tool that is planned but not yet implemented. Deferred entries
    /// document intended policy without being reported as unknown.
    #[serde(default)]
    pub deferred: bool,
}

/// Policy actually applied to a tool, after defaults are folded in.
#[derive(Debug, Clone, Copy)]
pub struct ToolPolicy {
    pub enabled: bool,
    pub requires_confirmation: bool,
    /// Per-tool override; `None` means use the limiter's default.
    pub rate_limit_per_minute: Option<u32>,
}

/// Parsed `config/tools.yaml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub defaults: ToolDefaults,
    #[serde(default)]
    pub tools: HashMap<String, ToolEntry>,
}

impl ToolsConfig {
    /// Environment override for the directory holding `tools.yaml`.
    pub const DIR_ENV: &'static str = "EMAILIBRIUM_CONFIG_DIR";

    /// Load `tools.yaml`, preferring [`DIR_ENV`](Self::DIR_ENV) over `fallback`.
    ///
    /// `fallback` is relative to the process working directory, which is only
    /// right when the binary is launched from `backend/`. Under a container
    /// with a different WORKDIR, a systemd unit, or `cargo run` from the repo
    /// root it resolves somewhere else — so operators get an absolute path via
    /// the environment.
    ///
    /// Missing the file is **loud**, at `error!` rather than `warn!`, because
    /// the fallback is maximally permissive: this file is the only place a tool
    /// can be disabled or given a tighter limit, so a wrong path silently
    /// re-enables everything an operator turned off. Startup still succeeds —
    /// a config problem should not cost the mailbox — but it cannot be quiet.
    pub fn load(fallback_dir: &str) -> Self {
        let dir = std::env::var(Self::DIR_ENV).unwrap_or_else(|_| fallback_dir.to_string());
        let path = std::path::Path::new(&dir).join("tools.yaml");

        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(e) => {
                tracing::error!(
                    "tools policy not found at {} ({e}) — FALLING BACK TO EVERY TOOL ENABLED at \
                     the default rate limit. Set {} to the directory holding tools.yaml.",
                    path.display(),
                    Self::DIR_ENV,
                );
                return Self::default();
            }
        };

        match serde_yaml::from_str::<Self>(&contents) {
            Ok(config) => {
                tracing::info!("Loaded tools policy: {}", path.display());
                config
            }
            Err(e) => {
                tracing::error!(
                    "failed to parse {}: {e} — FALLING BACK TO EVERY TOOL ENABLED at the default \
                     rate limit",
                    path.display(),
                );
                Self::default()
            }
        }
    }

    /// Resolved policy for `name`, falling back to defaults when unlisted.
    pub fn policy(&self, name: &str) -> ToolPolicy {
        match self.tools.get(name) {
            Some(entry) => ToolPolicy {
                enabled: entry.enabled.unwrap_or(true),
                requires_confirmation: entry.requires_confirmation.unwrap_or(false),
                rate_limit_per_minute: entry.rate_limit_per_minute,
            },
            None => ToolPolicy {
                enabled: true,
                requires_confirmation: false,
                rate_limit_per_minute: None,
            },
        }
    }

    /// Configured names with no matching tool, excluding deferred entries.
    pub fn unknown_tools<'a>(&self, known: impl Iterator<Item = &'a str>) -> Vec<String> {
        let known: Vec<&str> = known.collect();
        self.tools
            .iter()
            .filter(|(name, entry)| !entry.deferred && !known.contains(&name.as_str()))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(name: &str, entry: ToolEntry) -> ToolsConfig {
        let mut tools = HashMap::new();
        tools.insert(name.to_string(), entry);
        ToolsConfig {
            tools,
            ..ToolsConfig::default()
        }
    }

    #[test]
    fn unlisted_tools_are_enabled_by_default() {
        let policy = ToolsConfig::default().policy("search_emails");
        assert!(policy.enabled);
        assert!(!policy.requires_confirmation);
        assert_eq!(policy.rate_limit_per_minute, None);
    }

    #[test]
    fn explicit_entry_overrides_defaults() {
        let config = config_with(
            "send_email",
            ToolEntry {
                enabled: Some(false),
                requires_confirmation: Some(true),
                rate_limit_per_minute: Some(5),
                deferred: false,
            },
        );
        let policy = config.policy("send_email");
        assert!(!policy.enabled);
        assert!(policy.requires_confirmation);
        assert_eq!(policy.rate_limit_per_minute, Some(5));
    }

    #[test]
    fn unknown_tools_reports_entries_without_implementations() {
        let config = config_with("ghost_tool", ToolEntry::default());
        let unknown = config.unknown_tools(["search_emails"].into_iter());
        assert_eq!(unknown, vec!["ghost_tool".to_string()]);
    }

    #[test]
    fn deferred_entries_are_not_reported_as_unknown() {
        let config = config_with(
            "send_email",
            ToolEntry {
                deferred: true,
                ..ToolEntry::default()
            },
        );
        assert!(config
            .unknown_tools(["search_emails"].into_iter())
            .is_empty());
    }
}
