use std::collections::HashSet;
use std::path::{Path, PathBuf};

use aho_corasick::AhoCorasick;
use chrono::NaiveDate;

use crate::pii::redact;
use crate::pipeline::{attachments, timeline};
use crate::types::{Config, PiiFilter, Ticket, TimelineEntryKind};

/// Compute the output path for a ticket's markdown file.
pub fn output_path(output_dir: &Path, incident_number: &str, opened_date: &NaiveDate) -> PathBuf {
    let year = opened_date.format("%Y").to_string();
    let month = opened_date.format("%m").to_string();
    output_dir
        .join(&year)
        .join(&month)
        .join(incident_number)
        .join("ticket.md")
}

/// Export a ticket as markdown.
pub fn export(
    config: &Config,
    ticket: &mut Ticket,
    name_matcher: &Option<AhoCorasick>,
    pii_for_attachments: Option<(&Option<AhoCorasick>, bool)>,
) -> Result<(), String> {
    let ticket_dir = {
        let year = ticket.opened_date.format("%Y").to_string();
        let month = ticket.opened_date.format("%m").to_string();
        config
            .output_dir
            .join(&year)
            .join(&month)
            .join(&ticket.incident_number)
    };

    // Create output directory
    std::fs::create_dir_all(&ticket_dir).map_err(|e| {
        format!(
            "cannot create directory {}: {}",
            ticket_dir.display(),
            e
        )
    })?;

    // Resolve attachment filenames (reserved: "ticket.md")
    let reserved: HashSet<String> = ["ticket.md".to_string()].into_iter().collect();
    attachments::resolve_filenames(&mut ticket.attachments, &reserved);

    // Build timeline
    let timeline = timeline::build_timeline(
        std::mem::take(&mut ticket.messages),
        &ticket.attachments,
    );

    // Render markdown
    let mut md = String::new();

    // Header
    md.push_str("# ");
    md.push_str(&ticket.incident_number);
    if let Some(ref desc) = ticket.short_description {
        if !desc.is_empty() {
            md.push_str(" - ");
            md.push_str(&redact_markdown_field(
                desc,
                name_matcher,
                !matches!(config.pii_filter, PiiFilter::None),
                config.deterministic_pii,
            ));
        }
    }
    md.push('\n');

    // Metadata
    md.push('\n');
    md.push_str(&format!("- Status: {}\n", ticket.status));
    md.push_str(&format!("- Opened: {}\n", ticket.opened_date));
    if let Some(closed) = ticket.closed_date {
        md.push_str(&format!("- Closed: {}\n", closed));
    }

    // Timeline entries
    for entry in &timeline {
        md.push('\n');
        match &entry.kind {
            TimelineEntryKind::Message {
                author,
                text,
                internal,
            } => {
                md.push_str("## ");
                md.push_str(&redact_markdown_author(
                    author,
                    ticket.opener.as_deref(),
                    name_matcher,
                    !matches!(config.pii_filter, PiiFilter::None),
                    config.deterministic_pii,
                ));
                if *internal {
                    md.push_str(" (staff work notes)");
                }
                md.push('\n');
                md.push('\n');
                md.push_str(text);
                md.push('\n');
            }
            TimelineEntryKind::AttachmentGroup(files) => {
                md.push_str("## Attachments\n");
                md.push('\n');
                for file in files {
                    md.push_str(&format!("- `{}`\n", file));
                }
            }
        }
    }

    // Write ticket.md
    let ticket_path = ticket_dir.join("ticket.md");
    std::fs::write(&ticket_path, &md).map_err(|e| {
        format!("cannot write {}: {}", ticket_path.display(), e)
    })?;

    // Copy attachment files after writing ticket.md. If a copy fails, the ticket
    // markdown is left in place as partial output.
    attachments::copy_attachments(
        &ticket.attachments,
        &ticket_dir,
        config.symlink_attachments,
        pii_for_attachments,
    )?;

    Ok(())
}

fn redact_markdown_field(
    text: &str,
    name_matcher: &Option<AhoCorasick>,
    pii_enabled: bool,
    deterministic: bool,
) -> String {
    if !pii_enabled || !redact::might_contain_pii(text, name_matcher) {
        return text.to_string();
    }
    redact::redact_text(text, name_matcher, deterministic)
}

fn redact_markdown_author(
    author: &str,
    opener: Option<&str>,
    name_matcher: &Option<AhoCorasick>,
    pii_enabled: bool,
    deterministic: bool,
) -> String {
    let redacted = redact_markdown_field(author, name_matcher, pii_enabled, deterministic);
    if deterministic {
        return redacted;
    }

    let name_placeholder = match opener {
        Some(opener) if opener == author => "[ASKER]",
        _ => "[NAME]",
    };

    redacted.replace("[NAME]", name_placeholder)
}

#[cfg(test)]
mod tests {
    use super::{redact_markdown_author, redact_markdown_field};
    use aho_corasick::AhoCorasick;

    #[test]
    fn redacts_author_email_in_markdown_heading() {
        let matcher = Some(
            AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .build(["Adrian Hill"])
                .unwrap(),
        );
        let redacted = redact_markdown_author(
            "Adrian Hill (adrianhill@berkeley.edu)",
            Some("Adrian Hill (adrianhill@berkeley.edu)"),
            &matcher,
            true,
            false,
        );
        assert_eq!(redacted, "[ASKER] ([EMAIL])");
    }

    #[test]
    fn redacts_non_opener_author_name_in_markdown_heading() {
        let matcher = Some(
            AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .build(["Adrian Hill"])
                .unwrap(),
        );
        let redacted = redact_markdown_author(
            "Adrian Hill",
            Some("Someone Else"),
            &matcher,
            true,
            false,
        );
        assert_eq!(redacted, "[NAME]");
    }

    #[test]
    fn keeps_deterministic_author_aliases_in_markdown_heading() {
        let matcher = Some(
            AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .build(["Adrian Hill"])
                .unwrap(),
        );
        let redacted = redact_markdown_author(
            "Adrian Hill",
            Some("Adrian Hill"),
            &matcher,
            true,
            true,
        );
        assert_eq!(redacted, "USER_1AB03F1C8B");
    }

    #[test]
    fn redacts_ticket_title_metadata() {
        let redacted = redact_markdown_field(
            "Reactivate account yezhenyu@nersc.gov",
            &None,
            true,
            false,
        );
        assert_eq!(redacted, "Reactivate account [EMAIL]");
    }
}
