use std::collections::HashSet;
use std::path::Path;

use crate::types::Attachment;

/// Sanitize a filename for filesystem safety.
pub fn sanitize_filename(name: &str) -> String {
    let mut result: String = name
        .replace(['/', '\\'], "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-' || *c == ' ')
        .collect();

    // Strip leading/trailing spaces and dots
    result = result.trim_matches(|c: char| c == ' ' || c == '.').to_string();

    if result.is_empty() {
        "attachment".to_string()
    } else {
        result
    }
}

/// Ensure a filename is unique within a set. Adds numeric suffix on collision.
/// `reserved` contains filenames that must never be produced (e.g. "ticket.md" for markdown).
pub fn unique_filename(name: &str, used: &mut HashSet<String>, reserved: &HashSet<String>) -> String {
    let candidate = name.to_string();

    if !used.contains(&candidate) && !reserved.contains(&candidate) {
        used.insert(candidate.clone());
        return candidate;
    }

    // Split into stem and extension for suffix insertion
    let (stem, ext) = match name.rfind('.') {
        Some(dot) => (&name[..dot], &name[dot..]),
        None => (name, ""),
    };

    let mut counter = 2;
    loop {
        let candidate = format!("{}_{}{}", stem, counter, ext);
        if !used.contains(&candidate) && !reserved.contains(&candidate) {
            used.insert(candidate.clone());
            return candidate;
        }
        counter += 1;
    }
}

/// Resolve attachment filenames (sanitize + uniquify) and return the reserved filename set.
pub fn resolve_filenames(attachments: &mut [Attachment], reserved: &HashSet<String>) {
    let mut used = HashSet::new();
    for att in attachments.iter_mut() {
        let sanitized = sanitize_filename(&att.original_name);
        att.resolved_name = unique_filename(&sanitized, &mut used, reserved);
    }
}

/// Copy attachment files to the destination directory.
pub fn copy_attachments(attachments: &[Attachment], dest_dir: &Path) -> Result<(), String> {
    for att in attachments {
        let dest_path = dest_dir.join(&att.resolved_name);
        std::fs::copy(&att.local_path, &dest_path).map_err(|e| {
            format!(
                "failed to copy attachment '{}' from {} to {}: {}",
                att.original_name,
                att.local_path.display(),
                dest_path.display(),
                e
            )
        })?;
    }
    Ok(())
}
