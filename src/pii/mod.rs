pub mod attachments;
pub mod json;
pub mod redact;

use aho_corasick::AhoCorasick;

use crate::types::{PiiFilter, Ticket};

/// Build an Aho-Corasick automaton for known names/usernames from a ticket.
/// Returns None if the list is empty or the automaton cannot be built.
pub fn build_name_matcher(known_pii: &[String]) -> Option<AhoCorasick> {
    if known_pii.is_empty() {
        return None;
    }
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(known_pii)
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
