use std::sync::OnceLock;

use regex::Regex;

/// Clean a message's text according to the normalization rules.
/// Steps applied in order:
/// 1. Strip leading metadata lines
/// 2. Remove greeting line
/// 3. Remove trailing date/email-quote line
/// 4. Remove NERSC footer lines
/// 5. Remove signoff (+ optional trailing name)
/// 6. Remove author name lines
/// 7. Trim leading/trailing blank lines
pub fn clean_message_text(text: &str, author: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();

    strip_leading_metadata(&mut lines);
    remove_greeting(&mut lines);
    remove_trailing_date_line(&mut lines);
    remove_footer_lines(&mut lines);
    remove_signoff(&mut lines);
    remove_author_name_lines(&mut lines, author);
    trim_blank_lines(&mut lines);

    lines.join("\n")
}

// ── Step 1: Strip leading metadata ──────────────────────────────────────────

fn strip_leading_metadata(lines: &mut Vec<&str>) {
    while !lines.is_empty() {
        let first_non_empty = match lines.iter().position(|l| !l.trim().is_empty()) {
            Some(i) => i,
            None => break,
        };
        let trimmed = lines[first_non_empty].trim().to_ascii_lowercase();
        if trimmed.starts_with("reply from:")
            || trimmed.starts_with("created by:")
            || trimmed.starts_with("created by reply")
            || trimmed.starts_with("updated by reply")
        {
            lines.remove(first_non_empty);
        } else {
            break;
        }
    }
}

// ── Step 2: Remove greeting ─────────────────────────────────────────────────

fn greeting_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:(?:hi|hello|hey)(?:\s+\S+)?|dear\s+\S+|good\s+(?:morning|afternoon|evening)(?:\s+\S+)?)\s*[,!.]?\s*$"
        ).unwrap()
    })
}

fn remove_greeting(lines: &mut Vec<&str>) {
    if let Some(i) = lines.iter().position(|l| !l.trim().is_empty()) {
        if greeting_regex().is_match(lines[i].trim()) {
            lines.remove(i);
        }
    }
}

// ── Step 3: Remove trailing date line ───────────────────────────────────────

fn trailing_date_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\d{4}-\d{2}-\d{2}(?:\s+\d{2}:\d{2}:\d{2})?\s*(?:PDT|PST|UTC|GMT|ET|CT|MT|PT)?\s*$"
        ).unwrap()
    })
}

fn trailing_month_date_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:tember)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)\s+\d{1,2},?\s+\d{4}\s*(?:PDT|PST|UTC|GMT|ET|CT|MT|PT)?\s*$"
        ).unwrap()
    })
}

fn is_trailing_date_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() > 40 {
        return false;
    }
    trailing_date_regex().is_match(trimmed)
        || trailing_month_date_regex().is_match(trimmed)
}

fn is_email_quote_header(line: &str) -> bool {
    let trimmed = line.trim().to_ascii_lowercase();
    trimmed.starts_with("on ") && trimmed.contains(" at ")
}

fn remove_trailing_date_line(lines: &mut Vec<&str>) {
    if let Some(i) = lines.iter().rposition(|l| !l.trim().is_empty()) {
        let trimmed = lines[i].trim();
        if is_trailing_date_line(trimmed) || is_email_quote_header(trimmed) {
            lines.remove(i);
        }
    }
}

// ── Step 4: Remove footer lines ─────────────────────────────────────────────

fn footer_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:NERSC Account and Allocation Support\.?|NERSC Account & Allocations Support\.?|NERSC Consulting(?:\s*\|\s*User Engagement Group\s*\(UEG\))?\.?|NERSC User Engagement Group Lead\.?|NERSC Account Support:.*|accounts@nersc\.gov)\s*$"
        ).unwrap()
    })
}

fn strip_quote_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix('>') {
        rest.trim_start()
    } else {
        trimmed
    }
}

fn remove_footer_lines(lines: &mut Vec<&str>) {
    let re = footer_regex();
    lines.retain(|line| {
        let unquoted = strip_quote_prefix(line);
        !re.is_match(unquoted)
    });
}

// ── Step 5: Remove signoff ──────────────────────────────────────────────────

fn signoff_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^-{0,2}\s*(?:best regards|best wishes|kind regards|warm regards|many thanks|thank you|best|regards|cordially|thanks|sincerely|cheers)\s*[,.]?\s*$"
        ).unwrap()
    })
}

fn name_line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[A-Za-z][A-Za-z'\-.\s]{0,50}$").unwrap()
    })
}

fn is_name_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let word_count = trimmed.split_whitespace().count();
    word_count >= 1 && word_count <= 4 && name_line_regex().is_match(trimmed)
}

fn remove_signoff(lines: &mut Vec<&str>) {
    // Find last non-empty line
    let last_idx = match lines.iter().rposition(|l| !l.trim().is_empty()) {
        Some(i) => i,
        None => return,
    };

    let last_unquoted = strip_quote_prefix(lines[last_idx]);

    // Case 1: last non-empty line is a signoff
    if signoff_regex().is_match(last_unquoted) {
        lines.remove(last_idx);
        return;
    }

    // Case 2: last non-empty line is a name, and the previous non-empty line is a signoff
    if is_name_line(last_unquoted) {
        // Find second-to-last non-empty line
        let prev_idx = match lines[..last_idx].iter().rposition(|l| !l.trim().is_empty()) {
            Some(i) => i,
            None => return,
        };
        let prev_unquoted = strip_quote_prefix(lines[prev_idx]);
        if signoff_regex().is_match(prev_unquoted) {
            // Remove name line first (higher index), then signoff
            lines.remove(last_idx);
            lines.remove(prev_idx);
        }
    }
}

// ── Step 6: Remove author name lines ────────────────────────────────────────

fn remove_author_name_lines(lines: &mut Vec<&str>, author: &str) {
    if author.is_empty() {
        return;
    }

    let author_lower = author.to_lowercase();
    let parts: Vec<&str> = author.split_whitespace().collect();
    let first_name = parts.first().map(|s| s.to_lowercase());
    // "First L." variant: first name + last initial with period
    let initial_variant = if parts.len() >= 2 {
        let last = parts.last().unwrap();
        if let Some(ch) = last.chars().next() {
            Some(format!("{} {}.", parts[0], ch).to_lowercase())
        } else {
            None
        }
    } else {
        None
    };

    lines.retain(|line| {
        let unquoted = strip_quote_prefix(line).trim();
        if unquoted.is_empty() {
            return true;
        }
        let lower = unquoted.to_lowercase();
        // Check full name
        if lower == author_lower {
            return false;
        }
        // Check first name
        if let Some(ref first) = first_name {
            if &lower == first {
                return false;
            }
        }
        // Check "First L." variant
        if let Some(ref variant) = initial_variant {
            if &lower == variant {
                return false;
            }
        }
        true
    });
}

// ── Step 7: Trim blank lines ────────────────────────────────────────────────

fn trim_blank_lines(lines: &mut Vec<&str>) {
    // Trim leading blank lines (drain prefix in one operation)
    let first_non_blank = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(lines.len());
    if first_non_blank > 0 {
        lines.drain(..first_non_blank);
    }
    // Trim trailing blank lines
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
}
