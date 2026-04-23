//! Extractive passage retrieval for RAG context building (ADR-029).
//!
//! Instead of truncating email bodies to a fixed character limit, this module
//! extracts the most query-relevant sentences from each email body. This
//! maximizes information density within the LLM's context window.

use serde::{Deserialize, Serialize};

/// Configuration for extractive passage building.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractiveConfig {
    /// Whether to use extractive passages (vs. simple truncation).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Maximum passages (sentences) to extract per email.
    #[serde(default = "default_max_passages")]
    pub max_passages_per_email: usize,
    /// Minimum sentence length (chars) to consider.
    #[serde(default = "default_min_sentence")]
    pub min_sentence_chars: usize,
}

fn default_enabled() -> bool {
    false // off by default until tested
}
fn default_max_passages() -> usize {
    3
}
fn default_min_sentence() -> usize {
    20
}

impl Default for ExtractiveConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_passages_per_email: default_max_passages(),
            min_sentence_chars: default_min_sentence(),
        }
    }
}

/// A scored passage extracted from an email body.
#[derive(Debug, Clone)]
pub struct ExtractedPassage {
    pub text: String,
    #[allow(dead_code)]
    // Written during extraction; consumed by callers via destructuring, not field access
    pub score: f32,
    /// Original sentence index in the email body (for re-ordering).
    pub position: usize,
}

// ---------------------------------------------------------------------------
// Common abbreviations that should NOT trigger a sentence break.
// ---------------------------------------------------------------------------
const ABBREVIATIONS: &[&str] = &[
    "mr.", "mrs.", "ms.", "dr.", "prof.", "sr.", "jr.", "e.g.", "i.e.", "vs.", "etc.", "inc.",
    "ltd.", "co.", "u.s.", "u.k.", "a.m.", "p.m.", "no.", "vol.", "dept.", "est.", "approx.",
    "govt.", "assn.", "corp.", "jan.", "feb.", "mar.", "apr.", "jun.", "jul.", "aug.", "sep.",
    "oct.", "nov.", "dec.",
];

/// Split text into sentences using a rule-based approach.
///
/// Splits on sentence-ending punctuation (`.` `!` `?`) followed by whitespace
/// or end of string, while skipping common abbreviations (Mr., Dr., e.g., etc.).
pub fn split_sentences(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut sentences = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        current.push(chars[i]);

        if chars[i] == '.' || chars[i] == '!' || chars[i] == '?' {
            let lower_current = current.to_lowercase();
            // Check if the current text ends with a known abbreviation,
            // requiring a word boundary (space or start-of-string) before it
            // to avoid false matches like "test." matching "est.".
            let is_abbrev = ABBREVIATIONS.iter().any(|a| {
                if !lower_current.ends_with(a) {
                    return false;
                }
                let prefix_len = lower_current.len() - a.len();
                if prefix_len == 0 {
                    return true; // abbreviation is the entire token
                }
                let preceding = lower_current.as_bytes()[prefix_len - 1];
                preceding == b' ' || preceding == b'\t' || preceding == b'\n'
            });

            // Boundary: next char is whitespace, newline, or end of string.
            let at_boundary = i + 1 >= len || chars[i + 1].is_whitespace();

            if at_boundary && !is_abbrev && current.trim().len() >= 10 {
                sentences.push(current.trim().to_string());
                current = String::new();
                // Skip whitespace after the sentence terminator.
                while i + 1 < len && chars[i + 1].is_whitespace() {
                    i += 1;
                }
            }
        }

        i += 1;
    }

    // Push remaining text as a final sentence.
    let remaining = current.trim().to_string();
    if !remaining.is_empty() {
        sentences.push(remaining);
    }

    sentences
}

/// Score sentences by keyword overlap with the query.
///
/// Uses a simple TF-based scoring: fraction of query terms that appear in
/// the sentence. This avoids needing the embedding pipeline for per-sentence
/// scoring while still being effective for keyword-heavy email searches.
pub fn score_sentences_by_overlap(sentences: &[String], query: &str) -> Vec<(usize, f32)> {
    let query_terms: Vec<String> = query
        .split_whitespace()
        .map(|t| {
            t.to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
        .filter(|t| t.len() > 2) // skip short stop-words
        .collect();

    if query_terms.is_empty() {
        return sentences
            .iter()
            .enumerate()
            .map(|(i, _)| (i, 0.0))
            .collect();
    }

    sentences
        .iter()
        .enumerate()
        .map(|(i, sentence)| {
            let sentence_lower = sentence.to_lowercase();
            let matches = query_terms
                .iter()
                .filter(|term| sentence_lower.contains(term.as_str()))
                .count();
            let score = matches as f32 / query_terms.len() as f32;
            (i, score)
        })
        .collect()
}

/// Extract the most relevant passages from an email body.
///
/// Returns up to `max_passages` sentences sorted by relevance to the query,
/// then re-ordered by their original position for coherent reading. The total
/// extracted text is capped at `max_total_chars`.
pub fn extract_passages(
    body: &str,
    query: &str,
    max_passages: usize,
    min_sentence_chars: usize,
    max_total_chars: usize,
) -> Vec<ExtractedPassage> {
    let sentences = split_sentences(body);

    // Keep only sentences that meet the minimum length.
    let valid_sentences: Vec<(usize, &String)> = sentences
        .iter()
        .enumerate()
        .filter(|(_, s)| s.len() >= min_sentence_chars)
        .collect();

    if valid_sentences.is_empty() {
        // Fallback: return truncated body as a single passage.
        let truncated = if body.len() > max_total_chars {
            let end = body
                .char_indices()
                .nth(max_total_chars)
                .map(|(i, _)| i)
                .unwrap_or(body.len());
            format!("{}...", &body[..end])
        } else {
            body.to_string()
        };
        return vec![ExtractedPassage {
            text: truncated,
            score: 0.0,
            position: 0,
        }];
    }

    // Score each valid sentence against the query.
    let sentence_texts: Vec<String> = valid_sentences.iter().map(|(_, s)| (*s).clone()).collect();
    let scored = score_sentences_by_overlap(&sentence_texts, query);

    // Build (local_idx, orig_position, score) triples, sort by score desc.
    let mut scored_with_pos: Vec<(usize, usize, f32)> = scored
        .iter()
        .map(|(local_idx, score)| {
            let (orig_pos, _) = valid_sentences[*local_idx];
            (*local_idx, orig_pos, *score)
        })
        .collect();
    scored_with_pos.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Tie-break: prefer earlier sentences.
            .then(a.1.cmp(&b.1))
    });

    // Select top passages within the character budget.
    let mut selected = Vec::new();
    let mut total_chars = 0;
    for (local_idx, orig_pos, score) in &scored_with_pos {
        let sentence = valid_sentences[*local_idx].1;
        if total_chars + sentence.len() > max_total_chars && !selected.is_empty() {
            break;
        }
        if selected.len() >= max_passages {
            break;
        }
        total_chars += sentence.len();
        selected.push(ExtractedPassage {
            text: sentence.to_string(),
            score: *score,
            position: *orig_pos,
        });
    }

    // Re-sort by original position for coherent reading order.
    selected.sort_by_key(|p| p.position);
    selected
}

/// Format extracted passages for RAG context injection.
pub fn format_passages(passages: &[ExtractedPassage]) -> String {
    passages
        .iter()
        .map(|p| format!("  - {}", p.text))
        .collect::<Vec<_>>()
        .join("\n")
}

// ===========================================================================
// Tests
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    // ── split_sentences ──────────────────────────────────────────────────

    #[test]
    fn split_sentences_basic() {
        let text = "Hello world, this is a test. And here is another sentence. Finally done!";
        let result = split_sentences(text);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "Hello world, this is a test.");
        assert_eq!(result[1], "And here is another sentence.");
        assert_eq!(result[2], "Finally done!");
    }

    #[test]
    fn split_sentences_abbreviations() {
        let text = "Dr. Smith went to Washington. He met Mr. Jones there.";
        let result = split_sentences(text);
        // "Dr." and "Mr." should NOT cause splits.
        assert_eq!(result.len(), 2);
        assert!(result[0].contains("Dr. Smith"));
        assert!(result[1].contains("Mr. Jones"));
    }

    #[test]
    fn split_sentences_empty_input() {
        let result = split_sentences("");
        assert!(result.is_empty());
    }

    #[test]
    fn split_sentences_single_sentence() {
        let text = "Just one sentence without a terminator";
        let result = split_sentences(text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], text);
    }

    #[test]
    fn split_sentences_exclamation_and_question() {
        let text = "What is going on here? I have no idea! Let me check.";
        let result = split_sentences(text);
        assert_eq!(result.len(), 3);
        assert!(result[0].ends_with('?'));
        assert!(result[1].ends_with('!'));
        assert!(result[2].ends_with('.'));
    }

    #[test]
    fn split_sentences_eg_abbreviation() {
        let text = "We need items e.g. paper and pens for the office. Please order them today.";
        let result = split_sentences(text);
        // "e.g." should not cause a split.
        assert_eq!(result.len(), 2);
        assert!(result[0].contains("e.g."));
    }

    // ── score_sentences_by_overlap ───────────────────────────────────────

    #[test]
    fn score_perfect_match() {
        let sentences = vec!["The budget report is ready.".to_string()];
        let scores = score_sentences_by_overlap(&sentences, "budget report");
        assert_eq!(scores.len(), 1);
        assert!((scores[0].1 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn score_partial_match() {
        let sentences = vec!["The budget report is ready.".to_string()];
        let scores = score_sentences_by_overlap(&sentences, "budget meeting notes");
        assert_eq!(scores.len(), 1);
        // "budget" matches, "meeting" and "notes" don't -> 1/3
        assert!((scores[0].1 - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn score_no_match() {
        let sentences = vec!["The weather is sunny today.".to_string()];
        let scores = score_sentences_by_overlap(&sentences, "budget report");
        assert_eq!(scores.len(), 1);
        assert!((scores[0].1).abs() < f32::EPSILON);
    }

    #[test]
    fn score_case_insensitive() {
        let sentences = vec!["The BUDGET Report is ready.".to_string()];
        let scores = score_sentences_by_overlap(&sentences, "budget report");
        assert_eq!(scores.len(), 1);
        assert!((scores[0].1 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn score_empty_query() {
        let sentences = vec!["Some sentence here.".to_string()];
        let scores = score_sentences_by_overlap(&sentences, "");
        assert_eq!(scores.len(), 1);
        assert!((scores[0].1).abs() < f32::EPSILON);
    }

    #[test]
    fn score_short_query_words_filtered() {
        // Words <= 2 chars are filtered out, so "is" and "it" are skipped.
        let sentences = vec!["Is it done already with the project.".to_string()];
        let scores = score_sentences_by_overlap(&sentences, "is it done");
        assert_eq!(scores.len(), 1);
        // Only "done" survives filtering -> 1/1 = 1.0
        assert!((scores[0].1 - 1.0).abs() < f32::EPSILON);
    }

    // ── extract_passages ─────────────────────────────────────────────────

    #[test]
    fn extract_respects_max_passages() {
        let body = "First sentence is about budgets. \
                     Second sentence is about reports. \
                     Third sentence is about meetings. \
                     Fourth sentence is about travel.";
        let passages = extract_passages(body, "budgets reports meetings travel", 2, 10, 10000);
        assert!(passages.len() <= 2);
    }

    #[test]
    fn extract_respects_max_total_chars() {
        let body = "This is a fairly long sentence about the budget review process. \
                     Another sentence discusses the quarterly financial report in detail. \
                     A third sentence covers the upcoming board meeting schedule.";
        // Very tight budget: should stop after the first or second passage.
        let passages = extract_passages(body, "budget financial board", 10, 10, 80);
        let total: usize = passages.iter().map(|p| p.text.len()).sum();
        // The first selected passage may exceed the budget on its own, but
        // subsequent passages should not push far beyond.
        assert!(total <= 160, "total chars {total} is unreasonably large");
        assert!(!passages.is_empty());
    }

    #[test]
    fn extract_reorders_by_position() {
        // Sentence at position 2 matches best, sentence at position 0 matches second.
        let body = "The budget is important for planning. \
                     The weather is nice today. \
                     The budget review is scheduled for Friday.";
        let passages = extract_passages(body, "budget review", 2, 10, 10000);
        assert!(passages.len() == 2);
        // After re-ordering, the earlier sentence should come first.
        assert!(passages[0].position < passages[1].position);
    }

    #[test]
    fn extract_handles_empty_body() {
        let passages = extract_passages("", "budget", 3, 20, 1000);
        assert_eq!(passages.len(), 1);
        assert!(passages[0].text.is_empty());
    }

    #[test]
    fn extract_fallback_when_no_valid_sentences() {
        // Body is shorter than min_sentence_chars threshold.
        let passages = extract_passages("Short.", "budget", 3, 100, 1000);
        assert_eq!(passages.len(), 1);
        assert_eq!(passages[0].text, "Short.");
    }

    #[test]
    fn extract_truncates_fallback_body() {
        let body = "A".repeat(500);
        let passages = extract_passages(&body, "budget", 3, 600, 100);
        assert_eq!(passages.len(), 1);
        assert!(passages[0].text.ends_with("..."));
        // The truncated portion (before "...") should be around 100 chars.
        assert!(passages[0].text.len() <= 110);
    }

    // ── format_passages ──────────────────────────────────────────────────

    #[test]
    fn format_passages_bullet_points() {
        let passages = vec![
            ExtractedPassage {
                text: "First passage.".to_string(),
                score: 1.0,
                position: 0,
            },
            ExtractedPassage {
                text: "Second passage.".to_string(),
                score: 0.5,
                position: 1,
            },
        ];
        let formatted = format_passages(&passages);
        assert_eq!(formatted, "  - First passage.\n  - Second passage.");
    }

    #[test]
    fn format_passages_empty() {
        let formatted = format_passages(&[]);
        assert!(formatted.is_empty());
    }

    // ── ExtractiveConfig ─────────────────────────────────────────────────

    #[test]
    fn config_defaults() {
        let cfg = ExtractiveConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_passages_per_email, 3);
        assert_eq!(cfg.min_sentence_chars, 20);
    }

    #[test]
    fn config_serde_roundtrip() {
        let cfg = ExtractiveConfig {
            enabled: true,
            max_passages_per_email: 5,
            min_sentence_chars: 30,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: ExtractiveConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.max_passages_per_email, 5);
        assert_eq!(deserialized.min_sentence_chars, 30);
    }

    #[test]
    fn config_serde_defaults_applied() {
        let json = "{}";
        let cfg: ExtractiveConfig = serde_json::from_str(json).unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_passages_per_email, 3);
        assert_eq!(cfg.min_sentence_chars, 20);
    }
}
