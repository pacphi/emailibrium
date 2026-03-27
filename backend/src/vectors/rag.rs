//! RAG (Retrieval-Augmented Generation) pipeline for email-aware chat (ADR-022, DDD-010).
//!
//! Bridges the `HybridSearch` engine and the email database to provide
//! contextual email content for LLM prompts.  Provider-agnostic — the same
//! retrieval pipeline feeds context to the built-in LLM, Ollama, and cloud
//! providers.  The only variable is the token budget, which is caller-controlled.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::db::Database;

use super::error::VectorError;
use super::search::{HybridSearch, HybridSearchQuery, SearchMode};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// RAG pipeline configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagConfig {
    /// Maximum number of emails to retrieve per query.
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// Minimum relevance score (0.0–1.0) to include an email in context.
    #[serde(default = "default_min_score")]
    pub min_relevance_score: f32,
    /// Maximum approximate tokens to allocate for email context in the prompt.
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    /// Whether to include email body text (vs. metadata only).
    #[serde(default = "default_include_body")]
    pub include_body: bool,
    /// Maximum characters of body text per email.
    #[serde(default = "default_max_body_chars")]
    pub max_body_chars: usize,
}

fn default_top_k() -> usize {
    3
}
fn default_min_score() -> f32 {
    0.005 // RRF scores are 1/(k+rank) with k=60, so top results score ~0.016
}
fn default_max_context_tokens() -> usize {
    500 // Must fit within 2048-token context window alongside system prompt + history + response
}
fn default_include_body() -> bool {
    true
}
fn default_max_body_chars() -> usize {
    200
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            top_k: default_top_k(),
            min_relevance_score: default_min_score(),
            max_context_tokens: default_max_context_tokens(),
            include_body: default_include_body(),
            max_body_chars: default_max_body_chars(),
        }
    }
}

// ---------------------------------------------------------------------------
// RAG context (output)
// ---------------------------------------------------------------------------

/// Retrieval results formatted for prompt injection.
#[derive(Debug, Clone)]
pub struct RagContext {
    /// Pre-formatted text block ready to inject into the LLM prompt.
    pub formatted_context: String,
    /// IDs of the emails that were included.
    pub email_ids: Vec<String>,
    /// How many emails matched above the relevance threshold.
    pub result_count: usize,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Retrieval-Augmented Generation pipeline.
///
/// Given a user query, searches the email corpus via `HybridSearch`, fetches
/// matching email content from SQLite, and returns a token-budgeted context
/// block for prompt injection.
pub struct RagPipeline {
    search: Arc<HybridSearch>,
    db: Arc<Database>,
    config: RagConfig,
}

impl RagPipeline {
    pub fn new(search: Arc<HybridSearch>, db: Arc<Database>, config: RagConfig) -> Self {
        Self { search, db, config }
    }

    /// Retrieve email context relevant to the user's query.
    ///
    /// Returns an empty `RagContext` (with `result_count == 0`) when no emails
    /// match above `min_relevance_score`.
    pub async fn retrieve_context(
        &self,
        query: &str,
        max_context_tokens: Option<usize>,
    ) -> Result<RagContext, VectorError> {
        let budget = max_context_tokens.unwrap_or(self.config.max_context_tokens);

        // 1. Search emails using hybrid (semantic + keyword) search.
        let search_query = HybridSearchQuery {
            text: query.to_string(),
            mode: SearchMode::Hybrid,
            filters: None,
            limit: Some(self.config.top_k),
        };

        let search_result = self.search.search(&search_query).await?;

        debug!(
            query,
            total_results = search_result.results.len(),
            latency_ms = search_result.latency_ms,
            top_score = search_result
                .results
                .first()
                .map(|r| r.score)
                .unwrap_or(0.0),
            "RAG search completed"
        );

        // 2. Filter by minimum relevance score.
        let relevant: Vec<_> = search_result
            .results
            .into_iter()
            .filter(|r| r.score >= self.config.min_relevance_score)
            .collect();

        if relevant.is_empty() {
            debug!(query, "RAG: no emails matched above threshold");
            return Ok(RagContext {
                formatted_context: String::new(),
                email_ids: Vec::new(),
                result_count: 0,
            });
        }

        // 3. Fetch email content from the database.
        let email_ids: Vec<String> = relevant.iter().map(|r| r.email_id.clone()).collect();
        let emails = self.fetch_emails(&email_ids).await?;

        // 4. Format and fill context within token budget.
        let mut context_parts: Vec<String> = Vec::new();
        let mut used_ids: Vec<String> = Vec::new();
        let mut used_tokens: usize = 0;
        let budget_chars = budget * 4; // rough: 1 token ≈ 4 chars

        for email in &emails {
            let snippet = self.format_email(email);
            let snippet_chars = snippet.len();

            if used_tokens + (snippet_chars / 4) > budget {
                // Truncate this email's body to fit remaining budget
                let remaining_chars = budget_chars.saturating_sub(used_tokens * 4);
                if remaining_chars > 100 {
                    let truncated = self.format_email_truncated(email, remaining_chars);
                    context_parts.push(truncated);
                    used_ids.push(email.id.clone());
                }
                break;
            }

            used_tokens += snippet_chars / 4;
            used_ids.push(email.id.clone());
            context_parts.push(snippet);
        }

        let result_count = used_ids.len();
        let formatted_context = if context_parts.is_empty() {
            String::new()
        } else {
            format!(
                "The following {} email(s) are relevant to the user's question:\n\n{}",
                result_count,
                context_parts.join("\n\n")
            )
        };

        debug!(
            emails_used = result_count,
            tokens_used = used_tokens,
            "RAG context ready"
        );

        Ok(RagContext {
            formatted_context,
            email_ids: used_ids,
            result_count,
        })
    }

    /// Fetch email metadata and content from SQLite.
    async fn fetch_emails(&self, ids: &[String]) -> Result<Vec<EmailSnippet>, VectorError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Build a parameterized IN clause
        let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT id, subject, from_name, from_addr, received_at, body_text, category \
             FROM emails WHERE id IN ({}) ORDER BY received_at DESC",
            placeholders.join(", ")
        );

        let mut query = sqlx::query_as::<_, EmailRow>(&sql);
        for id in ids {
            query = query.bind(id);
        }

        let rows: Vec<EmailRow> = query
            .fetch_all(&self.db.pool)
            .await
            .map_err(|e| VectorError::EmbeddingFailed(format!("RAG email fetch failed: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| EmailSnippet {
                id: r.id,
                subject: r.subject,
                from_name: r.from_name,
                from_addr: r.from_addr,
                received_at: r.received_at,
                body_text: r.body_text,
                category: r.category,
            })
            .collect())
    }

    /// Format a single email as a compact context block.
    fn format_email(&self, email: &EmailSnippet) -> String {
        let sender = match &email.from_name {
            Some(name) if !name.is_empty() => format!("{name} <{}>", email.from_addr),
            _ => email.from_addr.clone(),
        };

        let mut s = format!(
            "--- Email ---\nFrom: {sender}\nSubject: {}\nDate: {}\nCategory: {}",
            email.subject, email.received_at, email.category,
        );

        if self.config.include_body {
            let body = email.body_text.as_deref().unwrap_or("");
            let body = if body.len() > self.config.max_body_chars {
                let truncated = &body[..body.floor_char_boundary(self.config.max_body_chars)];
                format!("{truncated}...")
            } else {
                body.to_string()
            };
            if !body.is_empty() {
                s.push_str(&format!("\nBody: {body}"));
            }
        }

        s
    }

    /// Format an email with a specific character budget.
    fn format_email_truncated(&self, email: &EmailSnippet, max_chars: usize) -> String {
        let sender = match &email.from_name {
            Some(name) if !name.is_empty() => format!("{name} <{}>", email.from_addr),
            _ => email.from_addr.clone(),
        };

        let header = format!(
            "--- Email ---\nFrom: {sender}\nSubject: {}\nDate: {}",
            email.subject, email.received_at,
        );

        if header.len() >= max_chars {
            return header[..header.floor_char_boundary(max_chars)].to_string();
        }

        let body_budget = max_chars - header.len() - 10; // 10 for "\nBody: " + "..."
        let body = email.body_text.as_deref().unwrap_or("");
        let body = if body.len() > body_budget {
            format!("{}...", &body[..body.floor_char_boundary(body_budget)])
        } else {
            body.to_string()
        };

        if body.is_empty() {
            header
        } else {
            format!("{header}\nBody: {body}")
        }
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct EmailSnippet {
    id: String,
    subject: String,
    from_name: Option<String>,
    from_addr: String,
    received_at: String,
    body_text: Option<String>,
    category: String,
}

/// Row type for sqlx query.
#[derive(Debug, sqlx::FromRow)]
struct EmailRow {
    id: String,
    subject: String,
    from_name: Option<String>,
    from_addr: String,
    received_at: String,
    body_text: Option<String>,
    category: String,
}
