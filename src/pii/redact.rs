use std::borrow::Cow;
use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use hmac::{Hmac, Mac};
use regex::{Regex, RegexSet};
use sha2::Sha256;

/// Fixed salt for deterministic HMAC pseudonymization.
/// Not for security — just for consistency across runs.
const HMAC_SALT: &[u8] = b"nersc-ticket-processor-v1";

fn merged_alternation(parts: &[&str]) -> String {
    format!("(?:{})", parts.join("|"))
}

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
            // Keep the pre-screen broad enough to catch likely phone shapes, but avoid
            // generic digit blobs that would trigger on timestamps, IDs, and job metadata.
            r"\+\d{1,3}[\s\-.]?\(?\d{2,4}\)?[\s\-.]?\d{3}[\s\-.]?\d{4}",
            r"\(\+\d{1,3}\)\s*\d{3,4}[\s\-.]?\d{3}[\s\-.]?\d{4}",
            r"\(\d{3}\)\s?\d{3}[\s\-.]?\d{4}",
            r"\b\d{3}[\-\.]\d{3}[\-\.]\d{4}\b",
            // Label-gated phone detection handles bare 10-digit numbers and spaced variants
            // only when they appear in phone-like fields such as "Work Phone Number:".
            r"(?i)(?:work\s+phone(?:\s+number)?|office\s+phone(?:\s+number)?|phone(?:\s+number)?|phone\s*#|cell|cel|mobile|tel|ph|fax|whatsapp)",
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
/// Applied in order: passwords, emails, username-in-context patterns, Zoom,
/// phones, then names.
/// Uses Cow<str> throughout to avoid allocations when regexes don't match.
pub(crate) fn redact_text(text: &str, name_matcher: &Option<AhoCorasick>, deterministic: bool) -> String {
    let text: Cow<str> = redact_passwords(text);
    let text: Cow<str> = redact_emails(&text, deterministic);
    let text: Cow<str> = redact_username_contexts(&text, deterministic);
    let text: Cow<str> = if might_contain_zoom(&text) {
        redact_zoom(&text)
    } else {
        text
    };
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
            r"\+\d{1,3}[\s\-.]?\(?\d{2,4}\)?[\s\-.]?\d{3}[\s\-.]?\d{4}",
            r"|",
            // (+91) 9444511865 or (+91) 9444 511 865
            r"\(\+\d{1,3}\)\s*\d{3,4}[\s\-.]?\d{3}[\s\-.]?\d{4}",
            r"|",
            // (555) 123-4567
            r"\(\d{3}\)\s?\d{3}[\s\-.]?\d{4}",
            r"|",
            // 555-123-4567 or 555.123.4567 (3-3-4 pattern with separators)
            r"\b\d{3}[\-\.]\d{3}[\-\.]\d{4}\b",
            r"|",
            // 555 123 4567, only when the groups are explicitly separated.
            // This stays out of the bare-10-digit case to avoid matching common IDs.
            r"\b\d{3}\s\d{3}\s\d{4}\b",
            r")",
        ))
        .unwrap()
    })
}

fn phone_label_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:work\s+phone(?:\s+number)?|office\s+phone(?:\s+number)?|phone(?:\s+number)?|phone\s*#|cell|cel|mobile|tel|ph|fax|whatsapp)",
        )
        .unwrap()
    })
}

fn labeled_phone_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let label = r"(?i:(?:work\s+phone(?:\s+number)?|office\s+phone(?:\s+number)?|phone(?:\s+number)?|phone\s*#|cell|cel|mobile|tel|ph|fax|whatsapp)\s*(?:is\s*)?[:=\-]?\s*)";
        let compact = r"(?:\(\+\d{1,3}\)\s*\d{7,11}|\+?\d{1,3}[\-\.]\d{1,4}[\-\.]\d{3,4}[\-\.]\d{4}|\d{10})\b";
        let spaced = r"(?:\(\+\d{1,3}\)\s*\d{3,4}[\s\-.]\d{3}[\s\-.]\d{4}|\+?\d{1,3}[\s\-.]\d{1,4}[\s\-.]\d{3,4}[\s\-.]\d{4}|\d{3}[\s\-.]\d{3}[\s\-.]\d{4})\b";
        let with_ext = r"(?:\+?\d{1,3}[\-\.]\d{1,4}[\-\.]\d{6,8}|\+?\d{1,3}[\s\-.]\d{1,4}[\s\-.]\d{6,8})(?:\s*(?:ext\.?|x)\s*\d{1,6})\b";
        let odd = r"\d[\d/\-]{7,}\d\b";
        let merged = format!("({}){}", label, merged_alternation(&[with_ext, spaced, compact, odd]));
        Regex::new(&merged).unwrap()
    })
}

fn redact_phones<'a>(text: &'a str) -> Cow<'a, str> {
    // Use a cheap label pre-check, then a merged regex built from narrower phone shapes.
    let text = if phone_label_regex().is_match(text) {
        Cow::Owned(
            labeled_phone_regex()
                .replace_all(text, "${1}[PHONE]")
                .into_owned(),
        )
    } else {
        Cow::Borrowed(text)
    };

    if phone_regex().is_match(&text) {
        Cow::Owned(phone_regex().replace_all(&text, "[PHONE]").into_owned())
    } else {
        text
    }
}

// ── Zoom meeting details ────────────────────────────────────────────────────
// Zoom links and meeting IDs are not useful in the exported corpus, and the
// one-tap mobile suffix is just another encoding of the same meeting details.

fn zoom_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let url = r"(?P<url>https?://[A-Za-z0-9.-]*zoom\.us/(?:j|meeting[s]?)/[^\s)>]+)";
        let meeting_id = r"(?P<meeting>(?P<meeting_prefix>(?i:meeting\s+id)\s*:\s*)\d(?:[\s-]?\d){8,})";
        let one_tap = r"(?P<one_tap>(?P<one_tap_prefix>,,)\d+#(?:,+\*\d+#)?)";
        let merged = merged_alternation(&[url, meeting_id, one_tap]);
        Regex::new(&merged).unwrap()
    })
}

fn might_contain_zoom(text: &str) -> bool {
    text.contains("zoom.us")
        || text.contains("Zoom")
        || text.contains("zoom")
        || text.contains("Meeting ID")
        || text.contains("meeting id")
        || text.contains(",,")
}

fn redact_zoom<'a>(text: &'a str) -> Cow<'a, str> {
    if zoom_regex().is_match(text) {
        Cow::Owned(
            zoom_regex()
                .replace_all(text, |caps: &regex::Captures| {
                    if caps.name("url").is_some() {
                        "[ZOOM]".to_string()
                    } else if let Some(prefix) = caps.name("meeting_prefix") {
                        format!("{}[ZOOM]", prefix.as_str())
                    } else if let Some(prefix) = caps.name("one_tap_prefix") {
                        format!("{}[ZOOM]", prefix.as_str())
                    } else {
                        "[ZOOM]".to_string()
                    }
                })
                .into_owned(),
        )
    } else {
        Cow::Borrowed(text)
    }
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
    if pos == 0 || pos == text.len() {
        return true;
    }
    let bytes = text.as_bytes();
    // A boundary is the position between bytes[pos - 1] and bytes[pos].
    // If either side is non-ASCII-alphanumeric, it counts as a word break.
    !bytes[pos - 1].is_ascii_alphanumeric() || !bytes[pos].is_ascii_alphanumeric()
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

#[cfg(test)]
mod tests {
    use super::redact_text;
    use aho_corasick::AhoCorasick;

    #[test]
    fn redacts_labeled_contiguous_ten_digit_phone() {
        let redacted = redact_text("Phone number - 6513410805", &None, false);
        assert_eq!(redacted, "Phone number - [PHONE]");
    }

    #[test]
    fn redacts_international_phone_with_four_digit_area_code() {
        let redacted = redact_text("+9 (0532) 596 1729", &None, false);
        assert_eq!(redacted, "[PHONE]");
    }

    #[test]
    fn redacts_labeled_spaced_phone() {
        let redacted = redact_text("Work Phone Number: 931 200 4393", &None, false);
        assert_eq!(redacted, "Work Phone Number: [PHONE]");
    }

    #[test]
    fn redacts_labeled_phone_with_is() {
        let redacted = redact_text("my phone number is 6466090124", &None, false);
        assert_eq!(redacted, "my phone number is [PHONE]");
    }

    #[test]
    fn redacts_whatsapp_phone() {
        let redacted = redact_text("WhatsApp: (+91) 9444511865", &None, false);
        assert_eq!(redacted, "WhatsApp: [PHONE]");
    }

    #[test]
    fn redacts_cel_phone() {
        let redacted = redact_text("Cel: 6314136020", &None, false);
        assert_eq!(redacted, "Cel: [PHONE]");
    }

    #[test]
    fn redacts_phone_with_extension() {
        let redacted = redact_text("Tel: +886-3-5780281 ext. 6307", &None, false);
        assert_eq!(redacted, "Tel: [PHONE]");
    }

    #[test]
    fn redacts_phone_with_odd_separators() {
        let redacted = redact_text("Tel: 1-865//591-4805", &None, false);
        assert_eq!(redacted, "Tel: [PHONE]");
    }

    #[test]
    fn redacts_ph_label() {
        let redacted = redact_text("Ph: 3034971289", &None, false);
        assert_eq!(redacted, "Ph: [PHONE]");
    }

    #[test]
    fn redacts_zoom_meeting_details() {
        let redacted = redact_text(
            "Join Zoom Meeting\nhttps://lbnl.zoom.us/j/7314990879?pwd=abc\nMeeting ID: 731 499 0879\n[PHONE],,7314990879#,,,,*760714# US (Los Angeles)",
            &None,
            false,
        );
        assert_eq!(
            redacted,
            "Join Zoom Meeting\n[ZOOM]\nMeeting ID: [ZOOM]\n[PHONE],,[ZOOM] US (Los Angeles)"
        );
    }

    #[test]
    fn redacts_names_and_emails_together() {
        let matcher = Some(
            AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .build(["Adrian Hill"])
                .unwrap(),
        );
        let redacted = redact_text("Adrian Hill <adrianhill@berkeley.edu>", &matcher, false);
        assert_eq!(redacted, "[NAME] <[EMAIL]>");
    }
}
