use aho_corasick::AhoCorasick;
use serde_json::Value;

use super::pii;

/// Structured user fields: replace entire value with USER_<HMAC10>.
const USER_FIELDS: &[&str] = &[
    "assigned_to",
    "caller_id",
    "closed_by",
    "created_by",
    "opened_by",
    "resolved_by",
    "reopened_by",
    "requested_for",
    "sys_created_by",
    "sys_updated_by",
    "u_owner",
    "u_user",
    "owner",
    "user",
];

/// Composite watch-list fields: parse comma-separated entries, replace each.
const WATCH_LIST_FIELDS: &[&str] = &[
    "u_itil_watch_list",
    "u_user_watchlist",
    "u_username_watchlist",
    "watch_list",
];

/// Email-specific fields: replace entire value with EMAIL_<HMAC10>.
const EMAIL_FIELDS: &[&str] = &[
    "email",
    "email_address",
    "u_email_watchlist",
    "u_email",
];

/// Recursively sanitize PII in a JSON value tree.
///
/// - Structured user fields → `USER_<HMAC10>`
/// - Email fields → `EMAIL_<HMAC10>`
/// - Watch-list fields → comma-separated aliases
/// - All other strings → free-text scan (emails, shell logins, paths, phones, passwords, names)
pub fn sanitize_value(value: &mut Value, name_matcher: &Option<AhoCorasick>) {
    match value {
        Value::Object(map) => {
            // Collect keys to process (we can't mutate while iterating)
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                let key_lower = key.to_lowercase();
                if USER_FIELDS.iter().any(|f| *f == key_lower) {
                    if let Some(val) = map.get_mut(&key) {
                        sanitize_user_field(val);
                    }
                } else if EMAIL_FIELDS.iter().any(|f| *f == key_lower) {
                    if let Some(val) = map.get_mut(&key) {
                        sanitize_email_field(val);
                    }
                } else if WATCH_LIST_FIELDS.iter().any(|f| *f == key_lower) {
                    if let Some(val) = map.get_mut(&key) {
                        sanitize_watch_list_field(val);
                    }
                } else {
                    // Recurse into child values
                    if let Some(val) = map.get_mut(&key) {
                        sanitize_value(val, name_matcher);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                sanitize_value(item, name_matcher);
            }
        }
        Value::String(s) => {
            *s = sanitize_free_text(s, name_matcher);
        }
        _ => {}
    }
}

/// Replace a structured user field value with USER_<HMAC10>.
fn sanitize_user_field(value: &mut Value) {
    match value {
        Value::String(s) => {
            if !s.is_empty() {
                *s = format!("USER_{}", pii::hmac_tag(s));
            }
        }
        // Wrapped {"display_value": ..., "value": ...} form
        Value::Object(map) => {
            for val in map.values_mut() {
                if let Value::String(s) = val {
                    if !s.is_empty() {
                        *s = format!("USER_{}", pii::hmac_tag(s));
                    }
                }
            }
        }
        _ => {}
    }
}

/// Replace an email field value with EMAIL_<HMAC10>.
fn sanitize_email_field(value: &mut Value) {
    match value {
        Value::String(s) => {
            if !s.is_empty() {
                *s = format!("EMAIL_{}", pii::hmac_tag(s));
            }
        }
        Value::Object(map) => {
            for val in map.values_mut() {
                if let Value::String(s) = val {
                    if !s.is_empty() {
                        *s = format!("EMAIL_{}", pii::hmac_tag(s));
                    }
                }
            }
        }
        _ => {}
    }
}

/// Parse comma-separated watch-list entries and replace each with USER_<HMAC10>.
fn sanitize_watch_list_field(value: &mut Value) {
    match value {
        Value::String(s) => {
            if !s.is_empty() {
                let sanitized: Vec<String> = s
                    .split(',')
                    .map(|entry| {
                        let entry = entry.trim();
                        if entry.is_empty() {
                            String::new()
                        } else {
                            format!("USER_{}", pii::hmac_tag(entry))
                        }
                    })
                    .filter(|e| !e.is_empty())
                    .collect();
                *s = sanitized.join(", ");
            }
        }
        Value::Object(map) => {
            for val in map.values_mut() {
                if let Value::String(s) = val {
                    if !s.is_empty() {
                        let sanitized: Vec<String> = s
                            .split(',')
                            .map(|entry| {
                                let entry = entry.trim();
                                if entry.is_empty() {
                                    String::new()
                                } else {
                                    format!("USER_{}", pii::hmac_tag(entry))
                                }
                            })
                            .filter(|e| !e.is_empty())
                            .collect();
                        *s = sanitized.join(", ");
                    }
                }
            }
        }
        _ => {}
    }
}

/// Apply free-text PII scanning to a string value.
/// Order: passwords, emails, shell logins, NERSC paths, command user flags, phones, names.
/// Uses Cow<str> / is_match guards to avoid allocations when regexes don't match.
fn sanitize_free_text(text: &str, name_matcher: &Option<AhoCorasick>) -> String {
    use std::borrow::Cow;

    let text: Cow<str> = pii::password_regex().replace_all(text, "${1}[PASSWORD]");

    let text: Cow<str> = pii::email_regex()
        .replace_all(&text, |caps: &regex::Captures| {
            format!("EMAIL_{}", pii::hmac_tag(&caps[0]))
        });

    // For the chained username-context regexes, use is_match guards to avoid
    // allocating when there's no match (the common case).
    let text: Cow<str> = if pii::shell_login_regex().is_match(&text) {
        Cow::Owned(pii::shell_login_regex()
            .replace_all(&text, |caps: &regex::Captures| {
                let user = &caps[1];
                let host = &caps[2];
                format!("USER_{}@{}", pii::hmac_tag(user), host)
            })
            .into_owned())
    } else {
        text
    };

    let text: Cow<str> = if pii::nersc_home_path_regex().is_match(&text) {
        Cow::Owned(pii::nersc_home_path_regex()
            .replace_all(&text, |caps: &regex::Captures| {
                let full = &caps[0];
                let username = caps
                    .get(1)
                    .or_else(|| caps.get(2))
                    .or_else(|| caps.get(3))
                    .unwrap()
                    .as_str();
                let prefix_end = full.len() - username.len();
                format!("{}USER_{}", &full[..prefix_end], pii::hmac_tag(username))
            })
            .into_owned())
    } else {
        text
    };

    let text: Cow<str> = if pii::command_user_flag_regex().is_match(&text) {
        Cow::Owned(pii::command_user_flag_regex()
            .replace_all(&text, |caps: &regex::Captures| {
                let full = &caps[0];
                let username = &caps[1];
                let prefix_end = full.len() - username.len();
                format!("{}USER_{}", &full[..prefix_end], pii::hmac_tag(username))
            })
            .into_owned())
    } else {
        text
    };

    let text: Cow<str> = pii::phone_regex().replace_all(&text, "[PHONE]");

    // Name dictionary matching
    redact_names_deterministic(&text, name_matcher)
}

/// Replace known names with USER_<HMAC10>, respecting word boundaries.
fn redact_names_deterministic(text: &str, matcher: &Option<AhoCorasick>) -> String {
    let ac = match matcher {
        Some(ac) => ac,
        None => return text.to_string(),
    };

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;

    for mat in ac.find_iter(text) {
        let start = mat.start();
        let end = mat.end();

        if is_word_boundary(text, start) && is_word_boundary(text, end) {
            result.push_str(&text[last_end..start]);
            let matched = &text[start..end];
            result.push_str(&format!("USER_{}", pii::hmac_tag(matched)));
            last_end = end;
        }
    }

    result.push_str(&text[last_end..]);
    result
}

fn is_word_boundary(text: &str, pos: usize) -> bool {
    if pos == 0 || pos >= text.len() {
        return true;
    }
    !text.as_bytes()[pos].is_ascii_alphanumeric()
}
