use std::collections::HashMap;
use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use serde_json::Value;

use super::redact;

/// How a recognized JSON field should be sanitized.
#[derive(Clone, Copy)]
enum FieldKind {
    User,
    Email,
    WatchList,
    /// Field value is structurally constrained (timestamp, ID, enum) and can
    /// never contain PII. Skip the free-text scan entirely.
    Skip,
}

/// Single HashMap for O(1) field classification instead of three linear scans.
fn field_kinds() -> &'static HashMap<&'static str, FieldKind> {
    static MAP: OnceLock<HashMap<&'static str, FieldKind>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::with_capacity(30);
        // Structured user fields: replace entire value with USER_<HMAC10>
        for f in [
            "assigned_to", "caller_id", "closed_by", "created_by", "opened_by",
            "resolved_by", "reopened_by", "requested_for", "sys_created_by",
            "sys_updated_by", "u_owner", "u_user", "owner", "user",
        ] {
            m.insert(f, FieldKind::User);
        }
        // Email-specific fields: replace entire value with EMAIL_<HMAC10>
        for f in ["email", "email_address", "u_email_watchlist", "u_email"] {
            m.insert(f, FieldKind::Email);
        }
        // Composite watch-list fields: parse comma-separated entries, replace each
        for f in [
            "u_itil_watch_list", "u_user_watchlist", "u_username_watchlist", "watch_list",
        ] {
            m.insert(f, FieldKind::WatchList);
        }
        // Timestamp fields: always date/datetime strings, never contain PII
        for f in [
            "opened_at", "closed_at", "resolved_at",
            "sys_created_on", "sys_updated_on",
            "u_resolved_at", "reopened_time",
        ] {
            m.insert(f, FieldKind::Skip);
        }
        // Incident ID and GUID fields: alphanumeric identifiers, never PII
        for f in ["number", "sys_id", "parent", "parent_incident"] {
            m.insert(f, FieldKind::Skip);
        }
        // Enum/status fields: short controlled-vocabulary values, never PII
        for f in [
            "state", "impact", "urgency", "priority",
            "contact_type", "close_code", "category", "subcategory",
            "approval", "active",
        ] {
            m.insert(f, FieldKind::Skip);
        }
        m
    })
}

/// Recursively sanitize PII in a JSON value tree.
///
/// - Structured user fields → `USER_<HMAC10>`
/// - Email fields → `EMAIL_<HMAC10>`
/// - Watch-list fields → comma-separated aliases
/// - All other strings → free-text scan (emails, shell logins, paths, phones, passwords, names)
pub fn sanitize_value(value: &mut Value, name_matcher: &Option<AhoCorasick>) {
    match value {
        Value::Object(map) => {
            let kinds = field_kinds();
            // Collect keys to process (we can't mutate while iterating)
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                let key_lower = key.to_lowercase();
                if let Some(val) = map.get_mut(&key) {
                    match kinds.get(key_lower.as_str()) {
                        Some(FieldKind::User) => sanitize_user_field(val),
                        Some(FieldKind::Email) => sanitize_email_field(val),
                        Some(FieldKind::WatchList) => sanitize_watch_list_field(val),
                        Some(FieldKind::Skip) => {}
                        None => sanitize_value(val, name_matcher),
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
            if redact::might_contain_pii(s, name_matcher) {
                *s = redact::redact_text(s, name_matcher, true);
            }
        }
        _ => {}
    }
}

/// Replace a structured user field value with USER_<HMAC10>.
fn sanitize_user_field(value: &mut Value) {
    match value {
        Value::String(s) => {
            if !s.is_empty() {
                *s = format!("USER_{}", redact::hmac_tag(s));
            }
        }
        // Wrapped {"display_value": ..., "value": ...} form
        Value::Object(map) => {
            for val in map.values_mut() {
                if let Value::String(s) = val
                    && !s.is_empty()
                {
                    *s = format!("USER_{}", redact::hmac_tag(s));
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
                *s = format!("EMAIL_{}", redact::hmac_tag(s));
            }
        }
        Value::Object(map) => {
            for val in map.values_mut() {
                if let Value::String(s) = val
                    && !s.is_empty()
                {
                    *s = format!("EMAIL_{}", redact::hmac_tag(s));
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
                            format!("USER_{}", redact::hmac_tag(entry))
                        }
                    })
                    .filter(|e| !e.is_empty())
                    .collect();
                *s = sanitized.join(", ");
            }
        }
        Value::Object(map) => {
            for val in map.values_mut() {
                if let Value::String(s) = val
                    && !s.is_empty()
                {
                    let sanitized: Vec<String> = s
                        .split(',')
                        .map(|entry| {
                            let entry = entry.trim();
                            if entry.is_empty() {
                                String::new()
                            } else {
                                format!("USER_{}", redact::hmac_tag(entry))
                            }
                        })
                        .filter(|e| !e.is_empty())
                        .collect();
                    *s = sanitized.join(", ");
                }
            }
        }
        _ => {}
    }
}
