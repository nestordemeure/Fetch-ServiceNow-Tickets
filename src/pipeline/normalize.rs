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
        let trimmed = normalized_for_matching(lines[first_non_empty]);
        if trimmed.starts_with("reply from ")
            || trimmed.starts_with("created by ")
            || trimmed.starts_with("created by reply")
            || trimmed.starts_with("updated by reply")
            || trimmed.starts_with("email received from ")
            || trimmed.starts_with("received from ")
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
            r"^(?:to whom it may concern|(?:(?:hi|hello|hey|dear|good (?:morning|afternoon|evening))(?: [a-z0-9]+){0,6}))$"
        ).unwrap()
    })
}

fn remove_greeting(lines: &mut Vec<&str>) {
    if let Some(i) = lines.iter().position(|l| !l.trim().is_empty()) {
        if greeting_regex().is_match(&normalized_for_matching(lines[i])) {
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
    let trimmed = normalized_for_matching(line);
    if !(trimmed.starts_with("on ") && trimmed.ends_with(" am"))
        && !(trimmed.starts_with("on ") && trimmed.ends_with(" pm"))
        && !(trimmed.starts_with("on ") && trimmed.contains(" at "))
    {
        return false;
    }

    trimmed.chars().any(|c| c.is_ascii_digit())
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
            r"^(?:nersc account and allocation support|nersc account allocations support|nersc consulting(?: user engagement group ueg)?|nersc user engagement group(?: lead)?|nersc data science engagement group|account and allocation support|allocations and account support|account support|nersc account support(?: .*)?|nersc user support team|the land i live and work on is the ancestral and unceded territory of the ohlone and bay miwok people|accounts nersc gov)$"
        ).unwrap()
    })
}

fn reply_header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:from|sent|to|cc|subject)(?: [a-z0-9]+)*$").unwrap()
    })
}

fn html_residue_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:<br\s*/?>|</?(?:blockquote|code|b|em)\b|\[/code\])").unwrap()
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
        !re.is_match(&normalized_for_matching(unquoted))
    });
}

// ── Step 5: Remove signoff ──────────────────────────────────────────────────

fn signoff_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:best regards|best wishes|kind regards|warm regards|many thanks(?: and best)?|thanks in advance|thanks and best|all the best|all best|thank you|best|regards|cordially|thanks|sincerely|cheers|bye)$"
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
    if matches!(trimmed.chars().last(), Some('.') | Some('!') | Some('?') | Some(':')) {
        return false;
    }
    let word_count = trimmed.split_whitespace().count();
    word_count >= 1 && word_count <= 4 && name_line_regex().is_match(trimmed)
}

fn signoff_with_name_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:best regards|best wishes|kind regards|warm regards|many thanks(?: and best)?|thanks in advance|thanks and best|all the best|all best|thank you|best|regards|cordially|thanks|sincerely|cheers|bye)(?: [a-z0-9]+){1,5}$"
        )
        .unwrap()
    })
}

fn signature_name_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^[a-z0-9]+(?: [a-z0-9]+){0,5}$"
        )
        .unwrap()
    })
}

fn signature_detail_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:(?:[a-z0-9]+ )*gender pronouns(?: [a-z0-9]+)*|(?:[a-z0-9]+ )*(?:email|phone)(?: [a-z0-9]+)*|(?:[a-z0-9]+ )*(?:national laboratory|department|division|chemistry|university|laboratory|lab|group|team|support|engagement|consulting|appointee|scientist|engineer|researcher|physicist|chemist|professor|postdoctoral|llnl)(?: [a-z0-9]+)*)$"
        )
        .unwrap()
    })
}

fn is_signature_name_line(line: &str) -> bool {
    let trimmed = line.trim();
    let normalized = normalized_for_matching(line);
    if trimmed.is_empty() || !signature_name_regex().is_match(&normalized) {
        return false;
    }

    trimmed.starts_with('-')
        || trimmed.contains('(')
        || trimmed.contains(',')
        || trimmed.contains('[')
}

fn is_trailing_artifact_line(line: &str) -> bool {
    let trimmed = line.trim();
    let normalized = normalized_for_matching(line);
    !trimmed.is_empty()
        && (reply_header_regex().is_match(&normalized)
            || html_residue_regex().is_match(trimmed))
}

fn is_signature_detail_line(line: &str) -> bool {
    let normalized = normalized_for_matching(line);
    if normalized.is_empty() {
        return false;
    }

    if footer_regex().is_match(&normalized) {
        return true;
    }

    (normalized.contains("gender pronouns")
        || normalized.split_whitespace().count() <= 6)
        && signature_detail_regex().is_match(&normalized)
}

fn has_preceding_signoff_context(lines: &[&str], start_idx: usize) -> bool {
    let mut idx = start_idx;

    while let Some(prev_idx) = lines[..idx].iter().rposition(|l| !l.trim().is_empty()) {
        let prev_unquoted = strip_quote_prefix(lines[prev_idx]).trim();
        let prev_normalized = normalized_for_matching(prev_unquoted);

        if signoff_regex().is_match(&prev_normalized) {
            return true;
        }

        if is_name_line(prev_unquoted)
            || is_signature_name_line(prev_unquoted)
            || is_signature_detail_line(prev_unquoted)
        {
            idx = prev_idx;
            continue;
        }

        return false;
    }

    false
}

fn remove_signoff(lines: &mut Vec<&str>) {
    let mut removed_trailing_signature = false;

    loop {
        let last_idx = match lines.iter().rposition(|l| !l.trim().is_empty()) {
            Some(i) => i,
            None => return,
        };
        let last_unquoted = strip_quote_prefix(lines[last_idx]).trim();
        let last_normalized = normalized_for_matching(last_unquoted);

        if is_trailing_artifact_line(last_unquoted) {
            lines.remove(last_idx);
            removed_trailing_signature = true;
            continue;
        }

        if signoff_regex().is_match(&last_normalized)
            || signoff_with_name_regex().is_match(&last_normalized)
        {
            lines.remove(last_idx);
            removed_trailing_signature = true;
            continue;
        }

        if is_name_line(last_unquoted) || is_signature_name_line(last_unquoted) {
            if removed_trailing_signature {
                lines.remove(last_idx);
                continue;
            }

            let prev_idx = match lines[..last_idx].iter().rposition(|l| !l.trim().is_empty()) {
                Some(i) => i,
                None => return,
            };
            let prev_unquoted = strip_quote_prefix(lines[prev_idx]).trim();
            if signoff_regex().is_match(&normalized_for_matching(prev_unquoted)) {
                lines.remove(last_idx);
                lines.remove(prev_idx);
                removed_trailing_signature = true;
                continue;
            }
        }

        if is_signature_detail_line(last_unquoted) {
            if removed_trailing_signature {
                lines.remove(last_idx);
                continue;
            }

            if has_preceding_signoff_context(lines, last_idx) {
                lines.remove(last_idx);
                removed_trailing_signature = true;
                continue;
            }
        }

        if removed_trailing_signature && is_signature_detail_line(last_unquoted) {
            lines.remove(last_idx);
            continue;
        }

        return;
    }
}

fn normalized_for_matching(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    let mut last_was_space = true;

    for ch in line.chars() {
        match ch {
            '<' => {
                in_tag = true;
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            '>' => in_tag = false,
            _ if in_tag => {}
            _ if ch.is_ascii_alphanumeric() => {
                out.push(ch.to_ascii_lowercase());
                last_was_space = false;
            }
            _ => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
        }
    }

    while out.ends_with(' ') {
        out.pop();
    }
    out
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

#[cfg(test)]
mod tests {
    use super::clean_message_text;

    #[test]
    fn removes_broader_leading_greetings() {
        let text = "To whom it may concern,\n\nMy password reset is failing.";
        assert_eq!(
            clean_message_text(text, ""),
            "My password reset is failing."
        );

        let text = "Dear NERSC Account Support Team,\n\nI need help logging in.";
        assert_eq!(clean_message_text(text, ""), "I need help logging in.");
    }

    #[test]
    fn removes_thanks_in_advance_signoff_and_footer_block() {
        let text = "My scratch files are gone.\n\nThanks in advance,\n\nJane Doe";
        assert_eq!(clean_message_text(text, ""), "My scratch files are gone.");

        let text = "I need help with my account.\n\nBest,\nJane Doe\nNERSC Data Science Engagement Group";
        assert_eq!(clean_message_text(text, ""), "I need help with my account.");
    }

    #[test]
    fn removes_inline_signoff_with_name() {
        let text = "The new link worked.\n\nBest, Joe";
        assert_eq!(clean_message_text(text, ""), "The new link worked.");

        let text = "The issue is fixed.\n\nBest, -Jane Doe";
        assert_eq!(clean_message_text(text, ""), "The issue is fixed.");
    }

    #[test]
    fn preserves_inline_thank_you_in_body_text() {
        let text = "Please let me know if this can be extended. Thanks in advance for your help.";
        assert_eq!(
            clean_message_text(text, ""),
            "Please let me know if this can be extended. Thanks in advance for your help."
        );
    }

    #[test]
    fn removes_multiline_signature_and_reply_headers() {
        let text = "I wanted to double check the issue.\n\nThanks,\n\n-- [NAME]\nFrom: [EMAIL] <[EMAIL]>";
        assert_eq!(clean_message_text(text, ""), "I wanted to double check the issue.");

        let text = "Please see the update below.\n\nThank you,\n\n[NAME] (Jason) Xu, Ph.D.\nPostdoctoral Appointee\nChemical Sciences and Engineering Division\nArgonne National Laboratory\nEmail: [EMAIL]\n\nFrom:";
        assert_eq!(clean_message_text(text, ""), "Please see the update below.");

        let text = "The issue is resolved.\n\nBest,\n\n\nOn 10/4/21 8:16 AM,";
        assert_eq!(clean_message_text(text, ""), "The issue is resolved.");

        let text = "That worked.\n\nThanks,\n\n\nOn 10/4/21 9:38 AM,";
        assert_eq!(clean_message_text(text, ""), "That worked.");
    }

    #[test]
    fn removes_signature_name_variants_and_html_residue() {
        let text = "The issue is fixed.\n\nSincerely,\n\n[NAME] (Gender pronouns: he/him/his)";
        assert_eq!(clean_message_text(text, ""), "The issue is fixed.");

        let text = "I have reset your AY 2022 ERCAP back to Draft so that you can update the CPU Node hours requested. You can just resubmit the form when you are done.\n\nSincerely,\n\nClayton Bagwell (Gender pronouns: he/him/his)\nAccount and Allocation Support";
        assert_eq!(
            clean_message_text(text, ""),
            "I have reset your AY 2022 ERCAP back to Draft so that you can update the CPU Node hours requested. You can just resubmit the form when you are done."
        );

        let text = "Please see my update below.\n\nThanks,\n[NAME]\nLLNL";
        assert_eq!(clean_message_text(text, ""), "Please see my update below.");

        let text = "I am following up on the chemistry environment.\n\nBest wishes,\n[NAME]\nBatista Lab & Pfefferle Lab\nDepartment of Chemistry\nYale University";
        assert_eq!(
            clean_message_text(text, ""),
            "I am following up on the chemistry environment."
        );

        let text = "The new process works.\n\nAll the best,\nChris<br /><b>Priority:   </b> Important</blockquote>[/code]";
        assert_eq!(clean_message_text(text, ""), "The new process works.");

        let text = "I can pick it up Wednesday.\n\nBye,";
        assert_eq!(clean_message_text(text, ""), "I can pick it up Wednesday.");

        let text = "The fix worked.\n\nThanks [NAME]!";
        assert_eq!(clean_message_text(text, ""), "The fix worked.");
    }

    #[test]
    fn removes_metadata_only_messages_after_normalized_matching() {
        let text = "reply from: [EMAIL]";
        assert_eq!(clean_message_text(text, ""), "");

        let text = "Email received from: [EMAIL]";
        assert_eq!(clean_message_text(text, ""), "");
    }
}
