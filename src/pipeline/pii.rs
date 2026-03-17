use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use regex::Regex;

use crate::types::{PiiFilter, Ticket};

/// Apply PII filtering to a ticket's messages.
///
/// Depending on `pii_filter`:
///   - `All`:   filter all messages.
///   - `Asker`: filter only messages from the ticket opener (non-staff).
///   - `None`:  no-op.
pub fn filter_pii(ticket: &mut Ticket, pii_filter: &PiiFilter) {
    if matches!(pii_filter, PiiFilter::None) {
        return;
    }

    // Build an Aho-Corasick automaton for known names/usernames from this ticket.
    let name_matcher = if !ticket.known_pii.is_empty() {
        AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&ticket.known_pii)
            .ok()
    } else {
        None
    };

    for msg in &mut ticket.messages {
        let should_filter = match pii_filter {
            PiiFilter::All => true,
            PiiFilter::Asker => match &ticket.opener {
                Some(opener) => msg.author == *opener,
                None => !msg.internal,
            },
            PiiFilter::None => unreachable!(),
        };

        if should_filter {
            msg.text = redact_text(&msg.text, &name_matcher);
        }
    }
}

/// Redact PII from a single text string.
/// Applied in order: passwords, emails, phones, then names.
fn redact_text(text: &str, name_matcher: &Option<AhoCorasick>) -> String {
    let text = redact_passwords(text);
    let text = redact_emails(&text);
    let text = redact_phones(&text);
    redact_names(&text, name_matcher)
}

// ── Emails ──────────────────────────────────────────────────────────────────

fn email_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap()
    })
}

fn redact_emails(text: &str) -> String {
    email_regex().replace_all(text, "[EMAIL]").into_owned()
}

// ── Phone numbers ───────────────────────────────────────────────────────────
// Conservative pattern: requires country code, parenthesized area code, or
// explicit separators in digit groups that look like phone numbers.
// Avoids matching dates (2025-05-08), node IDs (003417), or other numeric data.

fn phone_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?:",
            // +1 (555) 123-4567 or +1-555-123-4567
            r"\+\d{1,3}[\s\-.]?\(?\d{2,3}\)?[\s\-.]?\d{3}[\s\-.]?\d{4}",
            r"|",
            // (555) 123-4567
            r"\(\d{3}\)\s?\d{3}[\s\-.]?\d{4}",
            r"|",
            // 555-123-4567 or 555.123.4567 (3-3-4 pattern with separators)
            r"\b\d{3}[\-\.]\d{3}[\-\.]\d{4}\b",
            r")",
        ))
        .unwrap()
    })
}

fn redact_phones(text: &str) -> String {
    phone_regex().replace_all(text, "[PHONE]").into_owned()
}

// ── Passwords ───────────────────────────────────────────────────────────────

fn password_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)((?:pass(?:word|wd|code)?|pin|secret)\s*[:=]\s*)\S+").unwrap()
    })
}

fn redact_passwords(text: &str) -> String {
    password_regex()
        .replace_all(text, "${1}[PASSWORD]")
        .into_owned()
}

// ── Names (dictionary-based) ────────────────────────────────────────────────

/// Check if a byte position is at a word boundary (not adjacent to an alphanumeric char).
fn is_word_boundary(text: &str, pos: usize) -> bool {
    if pos == 0 || pos >= text.len() {
        return true;
    }
    let bytes = text.as_bytes();
    // Check the byte at the boundary: if it's part of a multi-byte UTF-8 sequence,
    // it's not an ASCII alphanumeric, so it's a boundary.
    !bytes[pos].is_ascii_alphanumeric()
}

fn redact_names(text: &str, matcher: &Option<AhoCorasick>) -> String {
    let ac = match matcher {
        Some(ac) => ac,
        None => return text.to_string(),
    };

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;

    for mat in ac.find_iter(text) {
        let start = mat.start();
        let end = mat.end();

        // Only replace if the match is at word boundaries on both sides
        if is_word_boundary(text, start) && is_word_boundary(text, end) {
            result.push_str(&text[last_end..start]);
            result.push_str("[NAME]");
            last_end = end;
        }
    }

    result.push_str(&text[last_end..]);
    result
}
