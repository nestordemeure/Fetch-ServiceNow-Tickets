use std::collections::HashSet;
use std::path::{Path, PathBuf};

use aho_corasick::AhoCorasick;
use chrono::NaiveDate;

use crate::pipeline::{attachments, timeline};
use crate::types::{Config, Ticket, TimelineEntryKind};

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
            md.push_str(desc);
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
                md.push_str(author);
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
