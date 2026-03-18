use std::collections::HashSet;
use std::path::Path;

use aho_corasick::AhoCorasick;
use rayon::prelude::*;

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

pub fn write_attachment(
    src: &Path,
    dest_path: &Path,
    description: &str,
    symlink_attachments: bool,
    pii: Option<(&Option<AhoCorasick>, bool)>,
) -> Result<(), String> {
    if std::fs::symlink_metadata(dest_path).is_ok() {
        std::fs::remove_file(dest_path).map_err(|e| {
            format!(
                "failed to remove existing output {} at {}: {}",
                description,
                dest_path.display(),
                e
            )
        })?;
    }

    // If PII redaction is enabled, try to redact text attachments first.
    // On success (PII found and redacted), we're done — skip normal copy/symlink.
    if let Some((name_matcher, deterministic)) = pii {
        if crate::pii::attachments::try_redact_text_attachment(
            src,
            dest_path,
            name_matcher,
            deterministic,
        )? {
            return Ok(());
        }
    }

    if symlink_attachments {
        std::os::unix::fs::symlink(src, dest_path).map_err(|e| {
            format!(
                "failed to symlink {} from {} to {}: {}",
                description,
                src.display(),
                dest_path.display(),
                e
            )
        })
    } else {
        std::fs::copy(src, dest_path)
            .map(|_| ())
            .map_err(|e| {
                format!(
                    "failed to copy {} from {} to {}: {}",
                    description,
                    src.display(),
                    dest_path.display(),
                    e
                )
            })
    }
}

/// Copy or symlink attachment files to the destination directory.
pub fn copy_attachments(
    attachments: &[Attachment],
    dest_dir: &Path,
    symlink_attachments: bool,
    pii: Option<(&Option<AhoCorasick>, bool)>,
) -> Result<(), String> {
    attachments.par_iter().try_for_each(|att| {
        let dest_path = dest_dir.join(&att.resolved_name);
        write_attachment(
            &att.local_path,
            &dest_path,
            &format!("attachment '{}'", att.original_name),
            symlink_attachments,
            pii,
        )
    })
}
