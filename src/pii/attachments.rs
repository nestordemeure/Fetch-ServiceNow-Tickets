use std::path::Path;

use aho_corasick::AhoCorasick;

use super::redact;

/// Extensions that are always binary — skip PII scanning entirely.
/// Based on actual extensions found in NERSC ServiceNow ticket attachments.
const BINARY_EXTENSIONS: &[&str] = &[
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
];

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
    name_matcher: &Option<AhoCorasick>,
    deterministic: bool,
) -> Result<bool, String> {
    // Fast path: skip known binary extensions without reading the file
    if let Some(ext) = src.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if BINARY_EXTENSIONS.iter().any(|b| *b == ext_lower) {
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

    let redacted = redact::redact_text(text, name_matcher, deterministic);

    // If nothing changed, no PII was found — let caller do normal copy/symlink
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
