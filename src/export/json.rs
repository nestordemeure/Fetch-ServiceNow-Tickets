use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde_json::Value;

use crate::pipeline::attachments;
use crate::pii;
use crate::pii::PiiMatchers;
use crate::types::Ticket;

/// Compute the output path for a ticket's JSON file.
/// Preserves input relative path under output_dir.
pub fn output_path(json_input_path: &Path, input_dir: &Path, output_dir: &Path) -> PathBuf {
    let rel = json_input_path
        .strip_prefix(input_dir)
        .unwrap_or(json_input_path.file_name().map(Path::new).unwrap_or(Path::new("unknown.json")));
    output_dir.join(rel)
}

/// Export a ticket as sanitized JSON.
pub fn export(
    ticket: &mut Ticket,
    json_input_path: &Path,
    input_dir: &Path,
    output_dir: &Path,
    symlink_attachments: bool,
    matchers: &PiiMatchers,
    pii_for_attachments: Option<(&PiiMatchers, bool)>,
) -> Result<(), String> {
    let raw_json = ticket
        .raw_json
        .take()
        .ok_or_else(|| format!("{}: raw_json not preserved", json_input_path.display()))?;

    let mut data = raw_json;

    // Step 1: Write processed messages back into raw JSON
    write_back_messages(&mut data, &ticket.messages);

    // Step 2: Recursive PII sanitization on the entire JSON tree
    pii::json::sanitize_value(&mut data, matchers);

    // Step 3: Copy or symlink attachment files, preserving relative paths
    write_attachments_json(&data, input_dir, output_dir, symlink_attachments, pii_for_attachments)?;

    // Step 4: Serialize with sorted keys + pretty-print
    let output = serialize_sorted(&data);

    // Step 5: Write JSON file
    let out_path = output_path(json_input_path, input_dir, output_dir);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!("cannot create directory {}: {}", parent.display(), e)
        })?;
    }
    std::fs::write(&out_path, &output).map_err(|e| {
        format!("cannot write {}: {}", out_path.display(), e)
    })?;

    Ok(())
}

/// Write processed message texts back into the raw JSON discussion arrays.
/// Removes entries that were filtered out (system messages, empty after normalize, dedup'd).
fn write_back_messages(data: &mut Value, messages: &[crate::types::Message]) {
    // Build lookup maps: source_index → processed text, for surviving messages
    let mut customer_map: HashMap<usize, &str> = HashMap::new();
    let mut internal_map: HashMap<usize, &str> = HashMap::new();

    for msg in messages {
        if let Some(idx) = msg.source_index {
            if msg.internal {
                internal_map.insert(idx, &msg.text);
            } else {
                customer_map.insert(idx, &msg.text);
            }
        }
    }

    // Update customer_facing_comments
    if let Some(comments) = data
        .get_mut("discussions")
        .and_then(|d| d.get_mut("customer_facing_comments"))
        .and_then(|c| c.as_array_mut())
    {
        let mut keep = Vec::new();
        for (i, entry) in comments.iter_mut().enumerate() {
            if let Some(text) = customer_map.get(&i) {
                if let Some(t) = entry.get_mut("text") {
                    *t = Value::String(text.to_string());
                }
                keep.push(i);
            }
        }
        // Remove entries not in the keep set (in reverse to preserve indices)
        let total = comments.len();
        for i in (0..total).rev() {
            if !keep.contains(&i) {
                comments.remove(i);
            }
        }
    }

    // Update internal_work_notes
    if let Some(notes) = data
        .get_mut("discussions")
        .and_then(|d| d.get_mut("internal_work_notes"))
        .and_then(|n| n.as_array_mut())
    {
        let mut keep = Vec::new();
        for (i, entry) in notes.iter_mut().enumerate() {
            if let Some(text) = internal_map.get(&i) {
                if let Some(t) = entry.get_mut("text") {
                    *t = Value::String(text.to_string());
                }
                keep.push(i);
            }
        }
        let total = notes.len();
        for i in (0..total).rev() {
            if !keep.contains(&i) {
                notes.remove(i);
            }
        }
    }
}

/// Copy or symlink attachment files from input to output, preserving relative paths.
fn write_attachments_json(
    data: &Value,
    input_dir: &Path,
    output_dir: &Path,
    symlink_attachments: bool,
    pii: Option<(&PiiMatchers, bool)>,
) -> Result<(), String> {
    let work: Vec<(PathBuf, PathBuf)> = match data.get("attachments").and_then(|a| a.as_array()) {
        Some(atts) => atts
            .iter()
            .filter_map(|att| {
                let local_path_str = att.get("local_path").and_then(|v| v.as_str())?;
                Some((input_dir.join(local_path_str), output_dir.join(local_path_str)))
            })
            .collect(),
        None => return Ok(()),
    };

    // Create parent directories up front (cheap, idempotent, avoids races)
    for (_, dst) in &work {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "cannot create attachment directory {}: {}",
                    parent.display(),
                    e
                )
            })?;
        }
    }

    // par_iter: attachment processing can be heavy (PII redaction reads,
    // regex-scans, and rewrites each text file).
    work.par_iter().try_for_each(|(src, dst)| {
        attachments::write_attachment(src, dst, "attachment", symlink_attachments, pii)
    })
}

/// Serialize a JSON value with sorted keys and 2-space indentation.
/// Matches Python's `json.dump(indent=2, sort_keys=True)`.
fn serialize_sorted(value: &Value) -> String {
    let mut buf = String::new();
    write_sorted_value(value, &mut buf, 0);
    buf.push('\n');
    buf
}

fn write_sorted_value(value: &Value, buf: &mut String, indent: usize) {
    match value {
        Value::Null => buf.push_str("null"),
        Value::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => buf.push_str(&n.to_string()),
        Value::String(s) => {
            buf.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => buf.push_str("\\\""),
                    '\\' => buf.push_str("\\\\"),
                    '\n' => buf.push_str("\\n"),
                    '\r' => buf.push_str("\\r"),
                    '\t' => buf.push_str("\\t"),
                    c if (c as u32) < 0x20 => {
                        buf.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => buf.push(c),
                }
            }
            buf.push('"');
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                buf.push_str("[]");
                return;
            }
            buf.push_str("[\n");
            let child_indent = indent + 2;
            for (i, item) in arr.iter().enumerate() {
                for _ in 0..child_indent {
                    buf.push(' ');
                }
                write_sorted_value(item, buf, child_indent);
                if i < arr.len() - 1 {
                    buf.push(',');
                }
                buf.push('\n');
            }
            for _ in 0..indent {
                buf.push(' ');
            }
            buf.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                buf.push_str("{}");
                return;
            }
            // Sort keys
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();

            buf.push_str("{\n");
            let child_indent = indent + 2;
            for (i, key) in keys.iter().enumerate() {
                for _ in 0..child_indent {
                    buf.push(' ');
                }
                // Write key
                buf.push('"');
                for ch in key.chars() {
                    match ch {
                        '"' => buf.push_str("\\\""),
                        '\\' => buf.push_str("\\\\"),
                        c => buf.push(c),
                    }
                }
                buf.push_str("\": ");
                write_sorted_value(&map[*key], buf, child_indent);
                if i < keys.len() - 1 {
                    buf.push(',');
                }
                buf.push('\n');
            }
            for _ in 0..indent {
                buf.push(' ');
            }
            buf.push('}');
        }
    }
}
