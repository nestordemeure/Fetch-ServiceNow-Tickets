use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use super::redact;
use super::PiiMatchers;

/// O(1) lookup for binary extensions instead of linear scan over ~50 entries.
fn binary_extensions() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            // Images
            "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "ico", "webp", "svg",
            // Documents (opaque binary formats)
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "rtf", "odt", "ods", "odp",
            // Archives / compressed
            "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "tgz",
            // Cryptographic
            "p7s",
            // Executables / libraries
            "exe", "dll", "so", "dylib", "o", "a",
            // Media
            "mp3", "mp4", "avi", "mov", "wav", "flac", "mkv", "wmv",
            // Fonts
            "woff", "woff2", "ttf", "otf", "eot",
            // Other binary
            "bin", "dat", "db", "sqlite", "class", "pyc", "pyo",
        ]
        .into_iter()
        .collect()
    })
}

/// Attempt to redact PII from a text-file attachment.
///
/// Returns:
/// - `Ok(true)` — file was text, PII was found, redacted content written to `dest`.
/// - `Ok(false)` — file is binary, not valid UTF-8, or contains no PII. Caller should
///   fall through to normal copy/symlink.
/// - `Err(...)` — I/O error reading/writing.
pub fn try_redact_text_attachment(
    src: &Path,
    dest: &Path,
    matchers: &PiiMatchers,
    deterministic: bool,
) -> Result<bool, String> {
    // Fast path: skip known binary extensions without reading the file
    if let Some(ext) = src.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if binary_extensions().contains(ext_lower.as_str()) {
            return Ok(false);
        }
    }

    let bytes = std::fs::read(src).map_err(|e| {
        format!("failed to read attachment {}: {}", src.display(), e)
    })?;

    // Try to interpret as UTF-8; fall through if not valid
    let text = match std::str::from_utf8(&bytes) {
        Ok(t) => t,
        Err(_) => return Ok(false),
    };

    // Fast pre-check: single RegexSet DFA pass + Aho-Corasick scan.
    // Avoids running 5+ individual regex replacements when no PII patterns exist.
    if !redact::might_contain_pii(text, matchers) {
        return Ok(false);
    }

    let redacted = redact::redact_text(text, matchers, deterministic);

    // Pre-check can have false positives (e.g. name match at non-word-boundary),
    // so verify something actually changed.
    if redacted == text {
        return Ok(false);
    }

    // PII was found: write redacted content
    std::fs::write(dest, redacted.as_bytes()).map_err(|e| {
        format!(
            "failed to write redacted attachment {}: {}",
            dest.display(),
            e
        )
    })?;

    Ok(true)
}
