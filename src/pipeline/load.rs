use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};
use serde_json::Value;

use crate::types::{Attachment, Message, OutputFormat, Ticket};

/// Read a JSON file from disk and parse it into a serde_json::Value.
pub fn read_json(path: &Path) -> Result<Value, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("{}: read error: {}", path.display(), e))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("{}: JSON parse error: {}", path.display(), e))
}

/// Load a Ticket from an already-parsed JSON Value.
pub fn load_ticket_from_value(data: Value, path: &Path, input_root: &Path, output_format: &OutputFormat) -> Result<Ticket, String> {
    let incident_number = require_json_str(&data, &["metadata", "incident_number"], path)?;
    let _number = require_json_str(&data, &["incident_fields", "number"], path)?;
    let status = require_json_str(&data, &["incident_fields", "state"], path)?;
    let short_description = optional_json_str(&data, &["incident_fields", "short_description"]);
    let opened_at_str = require_json_str(&data, &["incident_fields", "opened_at"], path)?;
    let closed_at_str = optional_json_str(&data, &["incident_fields", "closed_at"]);
    let contact_type = optional_json_str(&data, &["incident_fields", "contact_type"]);
    let close_code = optional_json_str(&data, &["incident_fields", "close_code"]);
    let created_by = optional_json_str(&data, &["incident_fields", "sys_created_by"]);
    let assignment_group = optional_json_str(&data, &["incident_fields", "assignment_group"]);

    let opened_date = parse_date(&opened_at_str)
        .map_err(|e| format!("{}: opened_at '{}': {}", path.display(), opened_at_str, e))?;
    let closed_date = match closed_at_str {
        Some(s) if !s.is_empty() => Some(
            parse_date(&s).map_err(|e| format!("{}: closed_at '{}': {}", path.display(), s, e))?,
        ),
        _ => None,
    };

    let discussions = &data["discussions"];
    let mut messages = Vec::new();

    let preserve_json = matches!(output_format, OutputFormat::Json);

    // Customer-facing comments (reverse chronological in JSON -> reverse to get chronological)
    if let Some(comments) = discussions["customer_facing_comments"].as_array() {
        for (i, msg) in comments.iter().enumerate() {
            let ctx = format!("{}:customer_facing_comments[{}]", path.display(), i);
            if let Some(mut m) = parse_message(msg, false, &ctx)? {
                if preserve_json {
                    m.source_index = Some(i);
                }
                messages.push(m);
            }
        }
        // Reverse: JSON is newest-first, we want oldest-first
        let comment_count = comments.len();
        let start = messages.len() - comment_count.min(messages.len());
        messages[start..].reverse();
    }

    // Internal work notes (same: reverse chronological)
    let notes_start = messages.len();
    if let Some(notes) = discussions["internal_work_notes"].as_array() {
        for (i, msg) in notes.iter().enumerate() {
            let ctx = format!("{}:internal_work_notes[{}]", path.display(), i);
            if let Some(mut m) = parse_message(msg, true, &ctx)? {
                if preserve_json {
                    m.source_index = Some(i);
                }
                messages.push(m);
            }
        }
        messages[notes_start..].reverse();
    }

    // Attachments
    let mut attachments = Vec::new();
    if let Some(atts) = data["attachments"].as_array() {
        for (i, att) in atts.iter().enumerate() {
            let ctx = format!("{}:attachments[{}]", path.display(), i);
            attachments.push(parse_attachment(att, input_root, &ctx)?);
        }
    }

    // Extract PII terms from ticket data
    let mut known_pii = Vec::new();
    collect_pii_terms(&data, &messages, &mut known_pii);

    // Identify opener: first customer-facing (non-internal) message author
    let opener = messages
        .iter()
        .find(|m| !m.internal)
        .map(|m| m.author.clone());

    let raw_json = if preserve_json { Some(data) } else { None };

    Ok(Ticket {
        incident_number,
        short_description,
        status,
        opened_date,
        closed_date,
        contact_type,
        close_code,
        created_by,
        assignment_group,
        messages,
        attachments,
        known_pii,
        opener,
        raw_json,
    })
}

/// Extract incident_number from an already-parsed JSON Value.
pub fn extract_incident_number(data: &Value, path: &Path) -> Result<String, String> {
    require_json_str(data, &["metadata", "incident_number"], path)
}

/// Extract opened_date from an already-parsed JSON Value.
pub fn extract_opened_date(data: &Value, path: &Path) -> Result<NaiveDate, String> {
    let opened_at_str = require_json_str(data, &["incident_fields", "opened_at"], path)?;
    parse_date(&opened_at_str)
        .map_err(|e| format!("{}: opened_at '{}': {}", path.display(), opened_at_str, e))
}

fn parse_message(msg: &Value, internal: bool, ctx: &str) -> Result<Option<Message>, String> {
    let author_raw = msg["created_by"]
        .as_str()
        .ok_or_else(|| format!("{}: missing 'created_by'", ctx))?;

    // Skip system messages
    if author_raw == "System" {
        return Ok(None);
    }

    let timestamp_str = msg["timestamp"]
        .as_str()
        .ok_or_else(|| format!("{}: missing 'timestamp'", ctx))?;
    let text = msg["text"]
        .as_str()
        .ok_or_else(|| format!("{}: missing 'text'", ctx))?;

    let timestamp = parse_datetime(timestamp_str)
        .map_err(|e| format!("{}: timestamp '{}': {}", ctx, timestamp_str, e))?;

    let author = normalize_author(author_raw, internal);

    Ok(Some(Message {
        author,
        timestamp,
        text: text.to_string(),
        internal,
        source_index: None,
    }))
}

fn parse_attachment(att: &Value, input_root: &Path, ctx: &str) -> Result<Attachment, String> {
    // Attachment fields are wrapped in {"display_value": ..., "value": ...}
    let file_name = att["file_name"]["value"]
        .as_str()
        .ok_or_else(|| format!("{}: missing 'file_name.value'", ctx))?
        .to_string();

    let timestamp_str = att["sys_created_on"]["value"]
        .as_str()
        .ok_or_else(|| format!("{}: missing 'sys_created_on.value'", ctx))?;
    let timestamp = parse_datetime(timestamp_str)
        .map_err(|e| format!("{}: sys_created_on '{}': {}", ctx, timestamp_str, e))?;

    let local_path_str = att["local_path"]
        .as_str()
        .ok_or_else(|| format!("{}: missing 'local_path'", ctx))?;
    let local_path = input_root.join(local_path_str);

    Ok(Attachment {
        original_name: file_name,
        resolved_name: String::new(), // set later by attachments module
        timestamp,
        local_path,
    })
}

/// Strip "(staff work notes (NERSC private))" or similar suffix from internal authors.
fn normalize_author(name: &str, internal: bool) -> String {
    if internal
        && let Some(idx) = name.find(" (")
    {
        return name[..idx].to_string();
    }
    name.to_string()
}

fn require_json_str(data: &Value, keys: &[&str], path: &Path) -> Result<String, String> {
    let mut current = data;
    for key in keys {
        current = &current[*key];
    }
    current.as_str().map(|s| s.to_string()).ok_or_else(|| {
        format!(
            "{}: missing required field '{}'",
            path.display(),
            keys.join(".")
        )
    })
}

fn optional_json_str(data: &Value, keys: &[&str]) -> Option<String> {
    let mut current = data;
    for key in keys {
        current = &current[*key];
    }
    current.as_str().map(|s| s.to_string())
}

fn parse_datetime(s: &str) -> Result<NaiveDateTime, String> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|e| format!("cannot parse datetime: {}", e))
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    // opened_at / closed_at are full timestamps, extract just the date
    let date_part = if let Some(idx) = s.find(' ') {
        &s[..idx]
    } else if let Some(idx) = s.find('T') {
        &s[..idx]
    } else {
        s
    };
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .map_err(|e| format!("cannot parse date: {}", e))
}

/// Extract names, usernames, and name parts from ticket data for PII dictionary matching.
///
/// Sources:
///   - message `created_by` fields (e.g. "First Last (username) (Staff work notes ...)")
///   - `incident_fields.caller_id` (e.g. "Last, First (username)")
///   - `incident_fields.opened_by` (e.g. "Last, First (username)")
///   - `incident_fields.sys_created_by` (plain username)
fn collect_pii_terms(data: &Value, messages: &[Message], out: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();

    let mut add = |term: &str| {
        let t = term.trim().to_string();
        // Minimum 3 characters to avoid short tokens matching inside common words
        if t.len() >= 3 && should_index_pii_term(&t) && seen.insert(t.to_lowercase()) {
            out.push(t);
        }
    };

    // From message authors (already normalized, but we also want the raw created_by for usernames)
    for msg in messages {
        add(&msg.author);
        // Individual name parts (first, last)
        for part in msg.author.split_whitespace() {
            add(part);
        }
    }

    // From incident_fields: caller_id, opened_by, sys_created_by
    let fields = &data["incident_fields"];
    for key in &["caller_id", "opened_by", "closed_by", "resolved_by"] {
        if let Some(raw) = fields[*key].as_str() {
            extract_name_and_username(raw, &mut add);
        }
    }
    if let Some(username) = fields["sys_created_by"].as_str() {
        add(username);
    }

    // From raw created_by in discussions (to capture usernames in parentheses)
    for section in &["customer_facing_comments", "internal_work_notes"] {
        if let Some(msgs) = data["discussions"][*section].as_array() {
            for msg in msgs {
                if let Some(raw) = msg["created_by"].as_str() {
                    extract_name_and_username(raw, &mut add);
                }
            }
        }
    }
}

fn should_index_pii_term(term: &str) -> bool {
    let trimmed = term.trim_matches(|c: char| !c.is_alphanumeric());
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_lowercase();
    if matches!(lower.as_str(), "system" | "guest" | "operator" | "support") {
        return false;
    }

    let letters: String = trimmed.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 3 && letters.chars().all(|c| c.is_uppercase()) {
        return false;
    }

    true
}

fn looks_like_person_name_component(component: &str) -> bool {
    let trimmed = component.trim();
    !trimmed.is_empty() && should_index_pii_term(trimmed)
}

/// Parse a name field like "Last, First (username)" or "First Last (username) (suffix)"
/// and call `add` for the full name, individual parts, and username.
fn extract_name_and_username(raw: &str, add: &mut impl FnMut(&str)) {
    if raw.is_empty() || raw == "System" {
        return;
    }

    // Extract username from first parenthesized group
    if let Some(paren_start) = raw.find('(') {
        let name_part = raw[..paren_start].trim();
        if !name_part.is_empty() {
            // Also add normalized "First Last" if comma-separated
            if let Some((last, first)) = name_part.split_once(',') {
                let first = first.trim();
                let last = last.trim();
                if looks_like_person_name_component(first) && looks_like_person_name_component(last) {
                    add(name_part);
                    add(&format!("{} {}", first, last));
                    add(first);
                    add(last);
                }
            } else {
                add(name_part);
                for part in name_part.split_whitespace() {
                    add(part);
                }
            }
        }

        // Username in parentheses
        if let Some(paren_end) = raw[paren_start..].find(')') {
            let username = &raw[paren_start + 1..paren_start + paren_end];
            add(username);
        }
    } else {
        // No parentheses - just a name or username
        add(raw);
        for part in raw.split_whitespace() {
            add(part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_name_and_username, should_index_pii_term};

    #[test]
    fn skips_role_and_org_name_parts() {
        let mut terms = Vec::new();
        extract_name_and_username("Operator, NERSC (operator)", &mut |term| terms.push(term.to_string()));
        assert_eq!(terms, vec!["operator".to_string()]);
    }

    #[test]
    fn rejects_all_caps_org_terms() {
        assert!(!should_index_pii_term("NERSC"));
        assert!(should_index_pii_term("Tony"));
    }
}
