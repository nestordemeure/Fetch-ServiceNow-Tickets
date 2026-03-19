pub mod attachments;
pub mod json;
pub mod redact;

use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};

use crate::types::{PiiFilter, Ticket};

/// Bundled Aho-Corasick matchers for the three name/ID categories.
pub struct PiiMatchers {
    /// Names belonging to the ticket opener → [ASKER] (non-det) / USER_<HMAC> (det).
    pub asker: Option<AhoCorasick>,
    /// All other known full names and name parts → [NAME] / USER_<HMAC>.
    pub names: Option<AhoCorasick>,
    /// Login IDs / usernames → [USERNAME] / USER_<HMAC>.
    pub usernames: Option<AhoCorasick>,
}

/// Build an Aho-Corasick automaton from a list of terms.
/// Returns None if the list is empty or the automaton cannot be built.
fn build_matcher(terms: &[String]) -> Option<AhoCorasick> {
    if terms.is_empty() {
        return None;
    }
    let mut patterns: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    patterns.sort_by_key(|s| std::cmp::Reverse(s.len()));
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .match_kind(MatchKind::LeftmostLongest)
        .kind(Some(AhoCorasickKind::DFA))
        .build(patterns)
        .ok()
}

/// Build the three PII matchers for a ticket.
///
/// - `asker`: built from the opener's name and name parts.
/// - `names`: built from `ticket.known_names`.
/// - `usernames`: built from `ticket.known_usernames`.
pub fn build_pii_matchers(ticket: &Ticket) -> PiiMatchers {
    // Build asker_terms from the opener's name.
    //
    // The raw opener string is the `created_by` field of the first customer message,
    // which often includes a parenthesized username: "Mahesh Natarajan (nataraj2)".
    // The text the asker writes in their messages uses just the name portion
    // ("Mahesh Natarajan"), so we must also add the stripped form.
    // With both the full form and the bare name in the automaton, LeftmostLongest
    // picks the longest match at each position, so "Mahesh Natarajan" in text is
    // matched as one unit → one [ASKER], not two.
    let asker_terms: Vec<String> = ticket
        .opener
        .as_deref()
        .map(|opener| {
            // Strip " (username)" suffix: the opener string is "Name (username)"
            // but message text only contains the name portion.
            let name_part = opener.find(" (").map_or(opener, |i| &opener[..i]);
            let mut terms = vec![name_part.to_string()];
            // Individual name components for first-name-only matches (e.g. "Hi Mahesh").
            for part in name_part.split_whitespace() {
                if part.len() >= 3 {
                    terms.push(part.to_string());
                }
            }
            terms
        })
        .unwrap_or_default();

    PiiMatchers {
        asker: build_matcher(&asker_terms),
        names: build_matcher(&ticket.known_names),
        usernames: build_matcher(&ticket.known_usernames),
    }
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
    matchers: &PiiMatchers,
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

        if should_filter && redact::might_contain_pii(&msg.text, matchers) {
            msg.text = redact::redact_text(&msg.text, matchers, deterministic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_matcher, PiiMatchers};
    use crate::pii::redact::redact_text;

    #[test]
    fn prefers_longest_name_match() {
        let terms = vec!["Tony".to_string(), "Quan".to_string(), "Tony Quan".to_string()];
        let matchers = PiiMatchers {
            asker: None,
            names: build_matcher(&terms),
            usernames: None,
        };
        let redacted = redact_text("Hi Tony Quan,", &matchers, false);
        assert_eq!(redacted, "Hi [NAME],");
    }
}
