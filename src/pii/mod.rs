pub mod attachments;
pub mod json;
pub mod redact;

use aho_corasick::{AhoCorasick, MatchKind};

use crate::types::{PiiFilter, Ticket};

/// Build an Aho-Corasick automaton for known names/usernames from a ticket.
/// Returns None if the list is empty or the automaton cannot be built.
pub fn build_name_matcher(known_pii: &[String]) -> Option<AhoCorasick> {
    if known_pii.is_empty() {
        return None;
    }
    let mut patterns: Vec<&str> = known_pii.iter().map(|s| s.as_str()).collect();
    patterns.sort_by_key(|s| std::cmp::Reverse(s.len()));
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .match_kind(MatchKind::LeftmostLongest)
        .build(patterns)
        .ok()
}

/// Apply PII filtering to a ticket's messages.
///
/// Depending on `pii_filter`:
///   - `All`:   filter all messages.
///   - `Asker`: filter only messages from the ticket opener (non-staff).
///   - `None`:  no-op.
pub fn filter_pii(
    ticket: &mut Ticket,
    pii_filter: &PiiFilter,
    name_matcher: &Option<AhoCorasick>,
    deterministic: bool,
) {
    if matches!(pii_filter, PiiFilter::None) {
        return;
    }

    for msg in &mut ticket.messages {
        let should_filter = match pii_filter {
            PiiFilter::All => true,
            PiiFilter::Asker => match &ticket.opener {
                Some(opener) => msg.author == *opener,
                None => !msg.internal,
            },
            PiiFilter::None => unreachable!(),
        };

        if should_filter && redact::might_contain_pii(&msg.text, name_matcher) {
            msg.text = redact::redact_text(&msg.text, name_matcher, deterministic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_name_matcher;
    use crate::pii::redact::redact_text;

    #[test]
    fn prefers_longest_name_match() {
        let terms = vec!["Tony".to_string(), "Quan".to_string(), "Tony Quan".to_string()];
        let matcher = build_name_matcher(&terms);
        let redacted = redact_text("Hi Tony Quan,", &matcher, false);
        assert_eq!(redacted, "Hi [NAME],");
    }
}
