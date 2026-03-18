use std::borrow::Cow;
use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use hmac::{Hmac, Mac};
use regex::{Regex, RegexSet};
use sha2::Sha256;

/// Fixed salt for deterministic HMAC pseudonymization.
/// Not for security — just for consistency across runs.
const HMAC_SALT: &[u8] = b"nersc-ticket-processor-v1";

// ── Deterministic hashing ─────────────────────────────────────────────────

pub(crate) fn hmac_tag(input: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(HMAC_SALT).expect("HMAC accepts any key length");
    mac.update(input.to_lowercase().as_bytes());
    let result = mac.finalize().into_bytes();
    // Take first 5 bytes = 10 hex chars
    hex::encode_upper(&result[..5])
}

/// Combined RegexSet for single-pass pre-screening of all PII patterns.
/// Used by `might_contain_pii` to avoid running individual replacements
/// when no patterns match at all — a single DFA pass instead of 5+ scans.
fn pii_regex_set() -> &'static RegexSet {
    static RS: OnceLock<RegexSet> = OnceLock::new();
    RS.get_or_init(|| {
        RegexSet::new([
            // email
            r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}",
            // phone patterns
            r"\+\d{1,3}[\s\-.]?\(?\d{2,3}\)?[\s\-.]?\d{3}[\s\-.]?\d{4}",
            r"\(\d{3}\)\s?\d{3}[\s\-.]?\d{4}",
            r"\b\d{3}[\-\.]\d{3}[\-\.]\d{4}\b",
            // password
            r"(?i)(?:pass(?:word|wd|code)?|pin|secret)\s*[:=]\s*\S+",
            // shell login (user@host) — overlaps with email, but both need checking
            r"[a-zA-Z][a-zA-Z0-9_]{1,31}@[a-zA-Z][a-zA-Z0-9.\-]+",
            // nersc paths
            r"/global/homes/[a-z]/[a-zA-Z][a-zA-Z0-9_]{1,31}",
            r"/pscratch/sd/[a-z]/[a-zA-Z][a-zA-Z0-9_]{1,31}",
            r"/global/cfs/cdirs/[a-zA-Z0-9_.\-]+/[a-zA-Z][a-zA-Z0-9_]{1,31}",
            // command user flag
            r"(?:--user\s+|-u\s+)[a-zA-Z][a-zA-Z0-9_]{1,31}\b",
        ])
        .unwrap()
    })
}

/// Fast pre-check: does this text contain any PII-like patterns?
/// Uses a single RegexSet DFA pass plus an Aho-Corasick check.
/// When this returns false, `redact_text` would return the input unchanged.
pub(crate) fn might_contain_pii(text: &str, name_matcher: &Option<AhoCorasick>) -> bool {
    pii_regex_set().is_match(text)
        || name_matcher.as_ref().is_some_and(|ac| ac.is_match(text))
}

/// Redact PII from a single text string.
/// Applied in order: passwords, emails, username-in-context patterns, phones, then names.
/// Uses Cow<str> throughout to avoid allocations when regexes don't match.
pub(crate) fn redact_text(text: &str, name_matcher: &Option<AhoCorasick>, deterministic: bool) -> String {
    let text: Cow<str> = redact_passwords(text);
    let text: Cow<str> = redact_emails(&text, deterministic);
    let text: Cow<str> = redact_username_contexts(&text, deterministic);
    let text: Cow<str> = redact_phones(&text);
    match redact_names(&text, name_matcher, deterministic) {
        Cow::Borrowed(s) => s.to_string(),
        Cow::Owned(s) => s,
    }
}

// ── Emails ──────────────────────────────────────────────────────────────────

pub(crate) fn email_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap()
    })
}

fn redact_emails<'a>(text: &'a str, deterministic: bool) -> Cow<'a, str> {
    if deterministic {
        email_regex()
            .replace_all(text, |caps: &regex::Captures| {
                format!("EMAIL_{}", hmac_tag(&caps[0]))
            })
    } else {
        email_regex().replace_all(text, "[EMAIL]")
    }
}

// ── Phone numbers ───────────────────────────────────────────────────────────
// Conservative pattern: requires country code, parenthesized area code, or
// explicit separators in digit groups that look like phone numbers.
// Avoids matching dates (2025-05-08), node IDs (003417), or other numeric data.

pub(crate) fn phone_regex() -> &'static Regex {
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

fn redact_phones<'a>(text: &'a str) -> Cow<'a, str> {
    phone_regex().replace_all(text, "[PHONE]")
}

// ── Passwords ───────────────────────────────────────────────────────────────

pub(crate) fn password_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)((?:pass(?:word|wd|code)?|pin|secret)\s*[:=]\s*)\S+").unwrap()
    })
}

fn redact_passwords<'a>(text: &'a str) -> Cow<'a, str> {
    password_regex().replace_all(text, "${1}[PASSWORD]")
}

// ── Username-in-context patterns ────────────────────────────────────────────
// Detect usernames embedded in shell logins, NERSC paths, and command flags.

pub(crate) fn shell_login_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // username@hostname-like (e.g. jsmith@perlmutter, user123@cori.nersc.gov)
    RE.get_or_init(|| {
        Regex::new(r"([a-zA-Z][a-zA-Z0-9_]{1,31})@([a-zA-Z][a-zA-Z0-9.\-]+)").unwrap()
    })
}

pub(crate) fn nersc_home_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // /global/homes/u/username, /pscratch/sd/u/username, /global/cfs/cdirs/project/username
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?:",
            r"/global/homes/[a-z]/([a-zA-Z][a-zA-Z0-9_]{1,31})",
            r"|",
            r"/pscratch/sd/[a-z]/([a-zA-Z][a-zA-Z0-9_]{1,31})",
            r"|",
            r"/global/cfs/cdirs/[a-zA-Z0-9_.\-]+/([a-zA-Z][a-zA-Z0-9_]{1,31})",
            r")",
        ))
        .unwrap()
    })
}

pub(crate) fn command_user_flag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // -u username or --user username (space-separated)
    RE.get_or_init(|| {
        Regex::new(r"(?:--user\s+|-u\s+)([a-zA-Z][a-zA-Z0-9_]{1,31})\b").unwrap()
    })
}

fn redact_username_contexts<'a>(text: &'a str, deterministic: bool) -> Cow<'a, str> {
    let placeholder = |username: &str| -> String {
        if deterministic {
            format!("USER_{}", hmac_tag(username))
        } else {
            "[NAME]".to_string()
        }
    };

    // Each step: only allocate (is_match + replace_all + into_owned) when there's a match.
    // Otherwise pass the Cow through unchanged — zero allocation for the common case.

    // Shell login: user@host → [NAME]@host or USER_HASH@host
    let text: Cow<'a, str> = if shell_login_regex().is_match(text) {
        Cow::Owned(shell_login_regex()
            .replace_all(text, |caps: &regex::Captures| {
                let user = &caps[1];
                let host = &caps[2];
                format!("{}@{}", placeholder(user), host)
            })
            .into_owned())
    } else {
        Cow::Borrowed(text)
    };

    // NERSC home paths: replace the username portion
    let text: Cow<'a, str> = if nersc_home_path_regex().is_match(&text) {
        Cow::Owned(nersc_home_path_regex()
            .replace_all(&text, |caps: &regex::Captures| {
                let full = &caps[0];
                let username = caps
                    .get(1)
                    .or_else(|| caps.get(2))
                    .or_else(|| caps.get(3))
                    .unwrap()
                    .as_str();
                let p = placeholder(username);
                let prefix_end = full.len() - username.len();
                format!("{}{}", &full[..prefix_end], p)
            })
            .into_owned())
    } else {
        text
    };

    // Command user flags: -u username or --user username
    if command_user_flag_regex().is_match(&text) {
        Cow::Owned(command_user_flag_regex()
            .replace_all(&text, |caps: &regex::Captures| {
                let full = &caps[0];
                let username = &caps[1];
                let p = placeholder(username);
                let prefix_end = full.len() - username.len();
                format!("{}{}", &full[..prefix_end], p)
            })
            .into_owned())
    } else {
        text
    }
}

// ── Names (dictionary-based) ────────────────────────────────────────────────

/// Check if a byte position is at a word boundary (not adjacent to an alphanumeric char).
pub(crate) fn is_word_boundary(text: &str, pos: usize) -> bool {
    if pos == 0 || pos >= text.len() {
        return true;
    }
    let bytes = text.as_bytes();
    // Check the byte at the boundary: if it's part of a multi-byte UTF-8 sequence,
    // it's not an ASCII alphanumeric, so it's a boundary.
    !bytes[pos].is_ascii_alphanumeric()
}

fn redact_names<'a>(text: &'a str, matcher: &Option<AhoCorasick>, deterministic: bool) -> Cow<'a, str> {
    let ac = match matcher {
        Some(ac) => ac,
        None => return Cow::Borrowed(text),
    };

    let mut result = String::new();
    let mut last_end = 0;
    let mut had_match = false;

    for mat in ac.find_iter(text) {
        let start = mat.start();
        let end = mat.end();

        // Only replace if the match is at word boundaries on both sides
        if is_word_boundary(text, start) && is_word_boundary(text, end) {
            if !had_match {
                result = String::with_capacity(text.len());
                had_match = true;
            }
            result.push_str(&text[last_end..start]);
            if deterministic {
                let matched = &text[start..end];
                result.push_str(&format!("USER_{}", hmac_tag(matched)));
            } else {
                result.push_str("[NAME]");
            }
            last_end = end;
        }
    }

    if had_match {
        result.push_str(&text[last_end..]);
        Cow::Owned(result)
    } else {
        Cow::Borrowed(text)
    }
}
