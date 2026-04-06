//! Query understanding layer for the enhanced RAG pipeline (ADR-029, Phase B).
//!
//! Translates natural language queries into structured [`ParsedQuery`] objects
//! that drive filter extraction, query-type routing, and hybrid search tuning.
//! The rule-based parser handles ~70-80% of queries in <1ms; a future LLM
//! fallback (Tier 2) will cover the remaining ambiguous cases.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::search::SearchFilters;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The structured output of the query understanding layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedQuery {
    /// Semantic text for vector search (structured parts stripped).
    pub semantic_text: Option<String>,
    /// Keywords for FTS5 search.
    pub fts_keywords: Option<String>,
    /// Structured filters extracted from the query.
    pub filters: SearchFilters,
    /// Type of query (determines routing strategy).
    pub query_type: QueryType,
    /// Confidence score from the parser (0.0-1.0).
    pub parse_confidence: f32,
    /// Which parser produced this result.
    pub parse_source: ParseSource,
}

/// Determines the search routing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryType {
    /// "what did Alice say about the budget?" -- hybrid search + re-rank.
    Factual,
    /// "find the email with invoice #12345" -- FTS5-dominant.
    NeedleInHaystack,
    /// "recent emails about project X" -- date-filtered + recency boost.
    Temporal,
    /// "how many emails from marketing this month?" -- SQL aggregation.
    Aggregation,
    /// "emails from Alice OR Bob about Q2" -- boolean parse + search.
    Boolean,
    /// Ambiguous / conversational -- full hybrid with HyDE.
    Semantic,
}

/// Identifies which parser produced the [`ParsedQuery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseSource {
    RuleBased,
    LlmConstrained,
    Hybrid,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a natural-language email search query into a structured [`ParsedQuery`].
///
/// The parser works in three stages:
/// 1. Extract structured filters (from, attachment, read status, temporal).
/// 2. Detect query type based on remaining tokens and original query.
/// 3. Compute confidence as `matched_chars / total_chars`.
pub fn parse_query(query: &str, now: DateTime<Utc>) -> ParsedQuery {
    let original = query.trim();
    if original.is_empty() {
        return ParsedQuery {
            semantic_text: None,
            fts_keywords: None,
            filters: SearchFilters::default(),
            query_type: QueryType::Semantic,
            parse_confidence: 0.0,
            parse_source: ParseSource::RuleBased,
        };
    }

    let total_chars = original.len() as f32;
    let mut matched_chars: usize = 0;
    let mut filters = SearchFilters::default();

    // Working copy that we progressively strip matched fragments from.
    let mut remainder = original.to_string();

    // ── Extract `from:X` / "from X" / "sent by X" ──────────────────────
    matched_chars += extract_senders(&mut remainder, &mut filters);

    // ── Extract attachment filters ──────────────────────────────────────
    matched_chars += extract_attachment(&mut remainder, &mut filters);

    // ── Extract read-status filters ─────────────────────────────────────
    matched_chars += extract_read_status(&mut remainder, &mut filters);

    // ── Extract temporal expressions ────────────────────────────────────
    matched_chars += extract_temporal(&mut remainder, &mut filters, now);

    // ── Detect query type ───────────────────────────────────────────────
    let query_type = detect_query_type(original, &remainder);

    // ── Build semantic remainder ────────────────────────────────────────
    let semantic = normalize_whitespace(&remainder);
    let semantic_text = if semantic.is_empty() {
        None
    } else {
        Some(semantic.clone())
    };
    let fts_keywords = semantic_text.clone();

    let parse_confidence = if total_chars > 0.0 {
        (matched_chars as f32 / total_chars).min(1.0)
    } else {
        0.0
    };

    ParsedQuery {
        semantic_text,
        fts_keywords,
        filters,
        query_type,
        parse_confidence,
        parse_source: ParseSource::RuleBased,
    }
}

// ---------------------------------------------------------------------------
// Filter extraction helpers
// ---------------------------------------------------------------------------

/// Extract sender filters from patterns like `from:alice`, `from alice`,
/// `sent by alice`.  Returns the number of matched characters removed.
/// Strip trailing punctuation from a captured sender name.
fn clean_sender(raw: &str) -> String {
    raw.trim_end_matches(['?', '!', '.', ',', ';', ':'])
        .to_string()
}

fn extract_senders(remainder: &mut String, filters: &mut SearchFilters) -> usize {
    let mut matched = 0usize;

    // `from:value` (no spaces around colon)
    let re_from_colon = Regex::new(r"(?i)\bfrom:(\S+)").unwrap();
    let captures: Vec<_> = re_from_colon
        .captures_iter(remainder)
        .map(|c| {
            let full = c.get(0).unwrap();
            let val = clean_sender(c.get(1).unwrap().as_str());
            (full.start(), full.end(), val)
        })
        .collect();
    for (start, end, val) in captures.iter().rev() {
        matched += end - start;
        filters
            .senders
            .get_or_insert_with(Vec::new)
            .push(val.clone());
        remainder.replace_range(start..end, "");
    }

    // "from <name>" — capture the first word plus any subsequent capitalized
    // words so that multi-word names like "Josh Bob" or "Mind Valley" are
    // captured as a single sender rather than splitting at the first space.
    // The `(?-i:...)` inline flag makes the uppercase check case-sensitive
    // within the otherwise case-insensitive regex.
    let re_from_word = Regex::new(r"(?i)\bfrom\s+(\S+(?:\s+(?-i:[A-Z])\S*)*)").unwrap();
    let captures: Vec<_> = re_from_word
        .captures_iter(remainder)
        .map(|c| {
            let full = c.get(0).unwrap();
            let val = clean_sender(c.get(1).unwrap().as_str());
            (full.start(), full.end(), val)
        })
        .collect();
    for (start, end, val) in captures.iter().rev() {
        matched += end - start;
        filters
            .senders
            .get_or_insert_with(Vec::new)
            .push(val.clone());
        remainder.replace_range(start..end, "");
    }

    // "sent by <name>" — same multi-word handling as "from".
    let re_sent_by = Regex::new(r"(?i)\bsent\s+by\s+(\S+(?:\s+(?-i:[A-Z])\S*)*)").unwrap();
    let captures: Vec<_> = re_sent_by
        .captures_iter(remainder)
        .map(|c| {
            let full = c.get(0).unwrap();
            let val = clean_sender(c.get(1).unwrap().as_str());
            (full.start(), full.end(), val)
        })
        .collect();
    for (start, end, val) in captures.iter().rev() {
        matched += end - start;
        filters
            .senders
            .get_or_insert_with(Vec::new)
            .push(val.clone());
        remainder.replace_range(start..end, "");
    }

    matched
}

/// Extract attachment-related filters.  Returns matched character count.
fn extract_attachment(remainder: &mut String, filters: &mut SearchFilters) -> usize {
    let patterns = [
        r"(?i)\bhas:attachment\b",
        r"(?i)\bwith\s+attachments?\b",
        r"(?i)\bwith\s+PDFs?\b",
    ];
    let mut matched = 0usize;
    for pat in &patterns {
        let re = Regex::new(pat).unwrap();
        if let Some(m) = re.find(remainder) {
            matched += m.end() - m.start();
            filters.has_attachment = Some(true);
            *remainder = re.replace(remainder, "").to_string();
        }
    }
    matched
}

/// Extract read-status filters.  Returns matched character count.
fn extract_read_status(remainder: &mut String, filters: &mut SearchFilters) -> usize {
    let patterns = [r"(?i)\bis:unread\b", r"(?i)\bunread\b"];
    let mut matched = 0usize;
    for pat in &patterns {
        let re = Regex::new(pat).unwrap();
        if let Some(m) = re.find(remainder) {
            matched += m.end() - m.start();
            filters.is_read = Some(false);
            *remainder = re.replace(remainder, "").to_string();
        }
    }
    matched
}

/// Extract temporal expressions and populate `date_from`/`date_to`.
/// Returns matched character count.
fn extract_temporal(
    remainder: &mut String,
    filters: &mut SearchFilters,
    now: DateTime<Utc>,
) -> usize {
    let mut matched = 0usize;

    // Ordered from most specific to least to avoid partial matches.
    let temporal_patterns: &[&str] = &[
        r"(?i)\blast\s+(\d+)\s+(days?|weeks?|months?)\b",
        r"(?i)\bin\s+(january|february|march|april|may|june|july|august|september|october|november|december)\b",
        r"(?i)\b(today|yesterday|this\s+week|last\s+week|this\s+month|last\s+month|recent)\b",
    ];

    for pat in temporal_patterns {
        let re = Regex::new(pat).unwrap();
        if let Some(cap) = re.captures(remainder) {
            let full = cap.get(0).unwrap();
            let expr = full.as_str();
            if let Some((from, to)) = resolve_temporal(expr, now) {
                matched += full.end() - full.start();
                filters.date_from = Some(from);
                filters.date_to = Some(to);
                *remainder = re.replace(remainder, "").to_string();
                break; // only extract the first temporal expression
            }
        }
    }

    matched
}

// ---------------------------------------------------------------------------
// Temporal resolution
// ---------------------------------------------------------------------------

/// Resolve a temporal expression relative to `now` into a (from, to) range.
pub fn resolve_temporal(expr: &str, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let lower = expr.trim().to_lowercase();
    // Collapse interior whitespace so "this  week" matches "this week".
    let normalized: String = lower.split_whitespace().collect::<Vec<_>>().join(" ");

    match normalized.as_str() {
        "today" => Some((start_of_day(now), now)),
        "yesterday" => {
            let y = now - Duration::days(1);
            Some((start_of_day(y), end_of_day(y)))
        }
        "this week" => Some((start_of_week(now), now)),
        "last week" => {
            let prev = now - Duration::weeks(1);
            Some((start_of_week(prev), end_of_week(prev)))
        }
        "this month" => Some((start_of_month(now), now)),
        "last month" => Some((start_of_prev_month(now), end_of_prev_month(now))),
        "recent" => Some((now - Duration::days(7), now)),
        _ => {
            // "last N days/weeks/months"
            if let Some(range) = resolve_last_n(&normalized, now) {
                return Some(range);
            }
            // "in <month>"
            if let Some(rest) = normalized.strip_prefix("in ") {
                if let Some(range) = resolve_month_name(rest.trim(), now) {
                    return Some(range);
                }
            }
            // Fallback to chrono-english
            chrono_english::parse_date_string(&normalized, now, chrono_english::Dialect::Us)
                .ok()
                .map(|d| (d, d))
        }
    }
}

/// Parse "last N days/weeks/months" patterns.
fn resolve_last_n(s: &str, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let re = Regex::new(r"^last\s+(\d+)\s+(days?|weeks?|months?)$").unwrap();
    let caps = re.captures(s)?;
    let n: i64 = caps.get(1)?.as_str().parse().ok()?;
    let unit = caps.get(2)?.as_str();
    let duration = if unit.starts_with("day") {
        Duration::days(n)
    } else if unit.starts_with("week") {
        Duration::weeks(n)
    } else if unit.starts_with("month") {
        Duration::days(n * 30) // approximation
    } else {
        return None;
    };
    Some((now - duration, now))
}

/// Resolve a month name (e.g. "march") to the first-last day of that month
/// in the current year.
fn resolve_month_name(name: &str, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let month_num = match name.to_lowercase().as_str() {
        "january" => 1u32,
        "february" => 2,
        "march" => 3,
        "april" => 4,
        "may" => 5,
        "june" => 6,
        "july" => 7,
        "august" => 8,
        "september" => 9,
        "october" => 10,
        "november" => 11,
        "december" => 12,
        _ => return None,
    };

    let year = now.year();
    let first = NaiveDate::from_ymd_opt(year, month_num, 1)?;
    let last = if month_num == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)? - Duration::days(1)
    } else {
        NaiveDate::from_ymd_opt(year, month_num + 1, 1)? - Duration::days(1)
    };

    let from = Utc.from_utc_datetime(&first.and_time(NaiveTime::from_hms_opt(0, 0, 0)?));
    let to = Utc.from_utc_datetime(&last.and_time(NaiveTime::from_hms_opt(23, 59, 59)?));
    Some((from, to))
}

// ---------------------------------------------------------------------------
// Date helpers
// ---------------------------------------------------------------------------

fn start_of_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.date_naive()
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
        .and_utc()
}

fn end_of_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.date_naive()
        .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
        .and_utc()
}

fn start_of_week(dt: DateTime<Utc>) -> DateTime<Utc> {
    let days_since_monday = dt.weekday().num_days_from_monday();
    let monday = dt - Duration::days(days_since_monday as i64);
    start_of_day(monday)
}

fn end_of_week(dt: DateTime<Utc>) -> DateTime<Utc> {
    let days_until_sunday = 6 - dt.weekday().num_days_from_monday();
    let sunday = dt + Duration::days(days_until_sunday as i64);
    end_of_day(sunday)
}

fn start_of_month(dt: DateTime<Utc>) -> DateTime<Utc> {
    let first = NaiveDate::from_ymd_opt(dt.year(), dt.month(), 1).unwrap();
    first
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
        .and_utc()
}

fn start_of_prev_month(dt: DateTime<Utc>) -> DateTime<Utc> {
    let (y, m) = if dt.month() == 1 {
        (dt.year() - 1, 12)
    } else {
        (dt.year(), dt.month() - 1)
    };
    let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    first
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
        .and_utc()
}

fn end_of_prev_month(dt: DateTime<Utc>) -> DateTime<Utc> {
    let last = NaiveDate::from_ymd_opt(dt.year(), dt.month(), 1).unwrap() - Duration::days(1);
    last.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
        .and_utc()
}

// ---------------------------------------------------------------------------
// Query type detection
// ---------------------------------------------------------------------------

fn detect_query_type(original: &str, _remainder: &str) -> QueryType {
    let lower = original.to_lowercase();

    // Aggregation keywords
    if lower.contains("how many")
        || lower.contains("count")
        || lower.contains("total")
        || lower.contains("number of")
    {
        return QueryType::Aggregation;
    }

    // Needle-in-haystack: quoted phrases, email addresses, invoice/order numbers
    if original.contains('"')
        || Regex::new(r"[\w.+-]+@[\w.-]+").unwrap().is_match(original)
        || Regex::new(r"(?i)(invoice|order|ticket|ref)\s*#?\d+")
            .unwrap()
            .is_match(original)
    {
        return QueryType::NeedleInHaystack;
    }

    // Boolean: explicit operators
    if Regex::new(r"\b(OR|AND|NOT)\b").unwrap().is_match(original) {
        return QueryType::Boolean;
    }

    // Temporal: primary axis is time
    let temporal_keywords = [
        "recent",
        "latest",
        "last week",
        "this week",
        "yesterday",
        "today",
        "this month",
        "last month",
        "last \\d+ days",
    ];
    for kw in &temporal_keywords {
        if Regex::new(&format!(r"(?i)\b{kw}\b"))
            .unwrap()
            .is_match(original)
        {
            return QueryType::Temporal;
        }
    }

    // Default: Factual
    QueryType::Factual
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Weekday};

    /// Fixed "now" for deterministic tests: Wednesday 2026-04-01 12:00:00 UTC.
    fn test_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap()
    }

    // ── Query type detection ────────────────────────────────────────────

    #[test]
    fn test_aggregation_detection() {
        let cases = [
            "how many emails from marketing",
            "count of unread emails",
            "total emails this month",
            "number of emails from alice",
        ];
        for q in &cases {
            let parsed = parse_query(q, test_now());
            assert_eq!(
                parsed.query_type,
                QueryType::Aggregation,
                "Expected Aggregation for: {q}"
            );
        }
    }

    #[test]
    fn test_needle_in_haystack_detection() {
        let cases = [
            r#"find "quarterly report""#,
            "email from user@example.com",
            "invoice #12345",
            "order 98765",
        ];
        for q in &cases {
            let parsed = parse_query(q, test_now());
            assert_eq!(
                parsed.query_type,
                QueryType::NeedleInHaystack,
                "Expected NeedleInHaystack for: {q}"
            );
        }
    }

    #[test]
    fn test_boolean_detection() {
        let q = "emails from Alice OR Bob about Q2";
        let parsed = parse_query(q, test_now());
        assert_eq!(parsed.query_type, QueryType::Boolean);
    }

    #[test]
    fn test_temporal_detection() {
        let cases = [
            "recent emails about deployments",
            "yesterday's standup notes",
        ];
        for q in &cases {
            let parsed = parse_query(q, test_now());
            assert_eq!(
                parsed.query_type,
                QueryType::Temporal,
                "Expected Temporal for: {q}"
            );
        }
    }

    #[test]
    fn test_factual_default() {
        let q = "what did alice say about the budget";
        let parsed = parse_query(q, test_now());
        assert_eq!(parsed.query_type, QueryType::Factual);
    }

    // ── Temporal resolution ─────────────────────────────────────────────

    #[test]
    fn test_resolve_today() {
        let now = test_now();
        let (from, to) = resolve_temporal("today", now).unwrap();
        assert_eq!(from, start_of_day(now));
        assert_eq!(to, now);
    }

    #[test]
    fn test_resolve_yesterday() {
        let now = test_now();
        let (from, to) = resolve_temporal("yesterday", now).unwrap();
        let y = now - Duration::days(1);
        assert_eq!(from, start_of_day(y));
        assert_eq!(to, end_of_day(y));
    }

    #[test]
    fn test_resolve_this_week() {
        let now = test_now(); // Wednesday 2026-04-01
        let (from, to) = resolve_temporal("this week", now).unwrap();
        // Monday 2026-03-30
        assert_eq!(from.weekday(), Weekday::Mon);
        assert_eq!(to, now);
    }

    #[test]
    fn test_resolve_last_week() {
        let now = test_now();
        let (from, to) = resolve_temporal("last week", now).unwrap();
        assert_eq!(from.weekday(), Weekday::Mon);
        assert_eq!(to.weekday(), Weekday::Sun);
        assert!(to < now);
    }

    #[test]
    fn test_resolve_this_month() {
        let now = test_now();
        let (from, to) = resolve_temporal("this month", now).unwrap();
        assert_eq!(from.day(), 1);
        assert_eq!(from.month(), 4);
        assert_eq!(to, now);
    }

    #[test]
    fn test_resolve_last_month() {
        let now = test_now(); // April 2026
        let (from, to) = resolve_temporal("last month", now).unwrap();
        assert_eq!(from.month(), 3);
        assert_eq!(from.day(), 1);
        assert_eq!(to.month(), 3);
        assert_eq!(to.day(), 31);
    }

    #[test]
    fn test_resolve_recent() {
        let now = test_now();
        let (from, to) = resolve_temporal("recent", now).unwrap();
        assert_eq!(to, now);
        assert_eq!(from, now - Duration::days(7));
    }

    #[test]
    fn test_resolve_last_n_days() {
        let now = test_now();
        let (from, to) = resolve_temporal("last 3 days", now).unwrap();
        assert_eq!(to, now);
        assert_eq!(from, now - Duration::days(3));
    }

    #[test]
    fn test_resolve_last_n_weeks() {
        let now = test_now();
        let (from, to) = resolve_temporal("last 2 weeks", now).unwrap();
        assert_eq!(to, now);
        assert_eq!(from, now - Duration::weeks(2));
    }

    #[test]
    fn test_resolve_in_march() {
        let now = test_now();
        let (from, to) = resolve_temporal("in march", now).unwrap();
        assert_eq!(from.month(), 3);
        assert_eq!(from.day(), 1);
        assert_eq!(to.month(), 3);
        assert_eq!(to.day(), 31);
    }

    #[test]
    fn test_resolve_in_february() {
        // 2026 is not a leap year
        let now = test_now();
        let (from, to) = resolve_temporal("in february", now).unwrap();
        assert_eq!(from.month(), 2);
        assert_eq!(from.day(), 1);
        assert_eq!(to.month(), 2);
        assert_eq!(to.day(), 28);
    }

    #[test]
    fn test_resolve_in_december() {
        let now = test_now();
        let (from, to) = resolve_temporal("in december", now).unwrap();
        assert_eq!(from.month(), 12);
        assert_eq!(from.day(), 1);
        assert_eq!(to.month(), 12);
        assert_eq!(to.day(), 31);
    }

    // ── Filter extraction ───────────────────────────────────────────────

    #[test]
    fn test_extract_from_colon() {
        let parsed = parse_query("from:alice budget review", test_now());
        let expected: Vec<String> = vec!["alice".to_string()];
        assert_eq!(parsed.filters.senders.as_deref(), Some(expected.as_slice()));
        // "budget review" should remain as semantic text
        assert!(parsed.semantic_text.as_ref().unwrap().contains("budget"));
    }

    #[test]
    fn test_extract_from_word() {
        let parsed = parse_query("emails from bob about project", test_now());
        let senders = parsed.filters.senders.unwrap();
        assert!(senders.iter().any(|s| s == "bob"));
    }

    #[test]
    fn test_extract_from_multi_word_name() {
        // Multi-word names with capitalized words should be captured together.
        let parsed = parse_query(
            "Did I receive any email from Josh Bob in the last 90 days?",
            test_now(),
        );
        let senders = parsed.filters.senders.unwrap();
        assert!(
            senders.iter().any(|s| s == "Josh Bob"),
            "Expected 'Josh Bob' in senders, got: {:?}",
            senders
        );
    }

    #[test]
    fn test_extract_sender_strips_trailing_punctuation() {
        // Trailing ? from the query should not be part of the sender name.
        let parsed = parse_query(
            "What are the subjects of last 3 emails received from Mind Valley?",
            test_now(),
        );
        let senders = parsed.filters.senders.unwrap();
        assert!(
            senders.iter().any(|s| s == "Mind Valley"),
            "Expected 'Mind Valley' (no trailing ?), got: {:?}",
            senders
        );
    }

    #[test]
    fn test_extract_sent_by() {
        let parsed = parse_query("sent by carol last week", test_now());
        let senders = parsed.filters.senders.unwrap();
        assert!(senders.iter().any(|s| s == "carol"));
    }

    #[test]
    fn test_extract_has_attachment() {
        let parsed = parse_query("has:attachment from alice", test_now());
        assert_eq!(parsed.filters.has_attachment, Some(true));
    }

    #[test]
    fn test_extract_with_attachments() {
        let parsed = parse_query("emails with attachments about invoices", test_now());
        assert_eq!(parsed.filters.has_attachment, Some(true));
    }

    #[test]
    fn test_extract_with_pdfs() {
        let parsed = parse_query("with PDFs from marketing", test_now());
        assert_eq!(parsed.filters.has_attachment, Some(true));
    }

    #[test]
    fn test_extract_is_unread() {
        let parsed = parse_query("is:unread emails from alice", test_now());
        assert_eq!(parsed.filters.is_read, Some(false));
    }

    #[test]
    fn test_extract_unread_keyword() {
        let parsed = parse_query("unread emails about budget", test_now());
        assert_eq!(parsed.filters.is_read, Some(false));
    }

    #[test]
    fn test_extract_temporal_this_month() {
        let now = test_now();
        let parsed = parse_query("emails from alice this month", now);
        assert!(parsed.filters.date_from.is_some());
        assert_eq!(parsed.filters.date_from.unwrap().month(), 4);
        assert_eq!(parsed.filters.date_from.unwrap().day(), 1);
    }

    #[test]
    fn test_extract_temporal_last_week() {
        let now = test_now();
        let parsed = parse_query("sent by bob last week", now);
        assert!(parsed.filters.date_from.is_some());
        assert!(parsed.filters.date_to.is_some());
        assert!(parsed.filters.date_to.unwrap() < now);
    }

    // ── Semantic remainder ──────────────────────────────────────────────

    #[test]
    fn test_semantic_remainder_strips_filters() {
        let parsed = parse_query("from:alice has:attachment budget review", test_now());
        let sem = parsed.semantic_text.unwrap();
        assert!(!sem.contains("from:alice"));
        assert!(!sem.contains("has:attachment"));
        assert!(sem.contains("budget"));
        assert!(sem.contains("review"));
    }

    #[test]
    fn test_semantic_remainder_empty_when_fully_parsed() {
        let parsed = parse_query("from:alice", test_now());
        // Only a filter, no semantic content remains.
        assert!(
            parsed.semantic_text.is_none() || parsed.semantic_text.as_deref() == Some(""),
            "Expected no semantic text, got: {:?}",
            parsed.semantic_text
        );
    }

    // ── Confidence scoring ──────────────────────────────────────────────

    #[test]
    fn test_confidence_fully_parsed() {
        // "from:alice" is entirely a filter, so confidence should be high.
        let parsed = parse_query("from:alice", test_now());
        assert!(
            parsed.parse_confidence > 0.5,
            "Expected high confidence, got {}",
            parsed.parse_confidence
        );
    }

    #[test]
    fn test_confidence_zero_for_plain_text() {
        // Plain semantic query with no extractable filters.
        let parsed = parse_query("what is the meaning of life", test_now());
        assert!(
            parsed.parse_confidence < 0.01,
            "Expected near-zero confidence for plain text, got {}",
            parsed.parse_confidence
        );
    }

    #[test]
    fn test_confidence_partial() {
        // "from:alice budget review" -- partial match
        let parsed = parse_query("from:alice budget review", test_now());
        assert!(parsed.parse_confidence > 0.0);
        assert!(parsed.parse_confidence < 1.0);
    }

    // ── Parse source ────────────────────────────────────────────────────

    #[test]
    fn test_parse_source_is_rule_based() {
        let parsed = parse_query("anything", test_now());
        assert_eq!(parsed.parse_source, ParseSource::RuleBased);
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn test_empty_query() {
        let parsed = parse_query("", test_now());
        assert_eq!(parsed.query_type, QueryType::Semantic);
        assert_eq!(parsed.parse_confidence, 0.0);
        assert!(parsed.semantic_text.is_none());
    }

    #[test]
    fn test_whitespace_only_query() {
        let parsed = parse_query("   ", test_now());
        assert_eq!(parsed.query_type, QueryType::Semantic);
        assert_eq!(parsed.parse_confidence, 0.0);
    }

    #[test]
    fn test_combined_filters() {
        let now = test_now();
        let parsed = parse_query(
            "unread emails from:alice with attachments last 5 days about budget",
            now,
        );
        assert_eq!(parsed.filters.is_read, Some(false));
        assert_eq!(parsed.filters.has_attachment, Some(true));
        assert!(parsed.filters.senders.is_some());
        assert!(parsed.filters.date_from.is_some());
        // Semantic remainder should contain "about budget" (and possibly "emails")
        let sem = parsed.semantic_text.unwrap();
        assert!(sem.contains("budget"), "Semantic text: {sem}");
    }
}
