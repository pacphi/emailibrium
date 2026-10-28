//! Gmail filter export (docs/plan/mail-agent-2026-10.md section 10,
//! milestone 4: "Gmail filter export").
//!
//! Pure XML generation for the Atom-feed format Gmail's Settings ->
//! Filters -> Import filters accepts. **Scope note:** there is no
//! persisted "this sender always gets this Kind" rule store yet — that's
//! the rules-engine persistence layer a later milestone builds on top of
//! `rules::authority`. This module exports whatever sender-rule set the
//! caller hands it (e.g. assembled from a `review`/`get_snapshot` pass
//! today, or a persisted rule set once one exists) — it is the format
//! generator, not a claim that a live "export my configured rules" tool
//! is wired to real stored rules yet.

use serde::{Deserialize, Serialize};

/// One sender rule to export, in the shape Gmail filters understand:
/// match a `from` address/domain, apply a label, optionally skip the
/// inbox (archive on arrival) — the two actions this plan's flow rules
/// actually need (section 8: "Export sender rules as provider filters...
/// so routing happens server-side").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderRuleExport {
    /// A `from:` match — a full sender key (`local-part@domain`) or a bare
    /// domain, matching whatever granularity the caller's rule used.
    pub from_match: String,
    pub apply_label: String,
    pub skip_inbox: bool,
}

/// Escape the handful of characters XML requires escaping in attribute
/// values. Not a general-purpose XML escaper — scoped to what a sender
/// address, domain or label name can actually contain.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Render a sender-rule set as Gmail's filter-import Atom XML.
pub fn to_gmail_filter_xml(rules: &[SenderRuleExport]) -> String {
    let mut entries = String::new();
    for rule in rules {
        entries.push_str(&format!(
            r#"  <entry>
    <category term='filter'></category>
    <title>Mail Filter</title>
    <content></content>
    <apps:property name='hasTheWord' value='from:({from})'/>
    <apps:property name='label' value='{label}'/>
    <apps:property name='shouldArchive' value='{archive}'/>
  </entry>
"#,
            from = xml_escape(&rule.from_match),
            label = xml_escape(&rule.apply_label),
            archive = rule.skip_inbox,
        ));
    }

    format!(
        r#"<?xml version='1.0' encoding='UTF-8'?>
<feed xmlns='http://www.w3.org/2005/Atom' xmlns:apps='http://schemas.google.com/apps/2006'>
  <title>Mail Filters</title>
{entries}</feed>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rule_set_produces_a_valid_feed_with_no_entries() {
        let xml = to_gmail_filter_xml(&[]);
        assert!(xml.contains("<feed"));
        assert!(!xml.contains("<entry>"));
    }

    #[test]
    fn one_rule_round_trips_its_fields_into_the_expected_properties() {
        let rules = vec![SenderRuleExport {
            from_match: "github.com".to_string(),
            apply_label: "Kind/Notifications".to_string(),
            skip_inbox: true,
        }];
        let xml = to_gmail_filter_xml(&rules);
        assert!(xml.contains("from:(github.com)"));
        assert!(xml.contains("value='Kind/Notifications'"));
        assert!(xml.contains("value='true'"));
    }

    #[test]
    fn special_characters_in_a_sender_or_label_are_escaped() {
        let rules = vec![SenderRuleExport {
            from_match: "a&b@example.com".to_string(),
            apply_label: "Kind/\"Quoted\"".to_string(),
            skip_inbox: false,
        }];
        let xml = to_gmail_filter_xml(&rules);
        assert!(xml.contains("a&amp;b@example.com"));
        assert!(xml.contains("&quot;Quoted&quot;"));
        assert!(
            !xml.contains("a&b@example.com"),
            "raw ampersand must not survive unescaped"
        );
    }

    #[test]
    fn multiple_rules_each_get_their_own_entry() {
        let rules = vec![
            SenderRuleExport {
                from_match: "github.com".into(),
                apply_label: "Kind/Notifications".into(),
                skip_inbox: true,
            },
            SenderRuleExport {
                from_match: "amazon.com".into(),
                apply_label: "Kind/Receipts".into(),
                skip_inbox: true,
            },
        ];
        let xml = to_gmail_filter_xml(&rules);
        assert_eq!(xml.matches("<entry>").count(), 2);
    }
}
