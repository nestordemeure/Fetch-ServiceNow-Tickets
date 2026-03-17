pub mod attachments;
pub mod dedup;
pub mod filter;
pub mod load;
pub mod normalize;
pub mod timeline;

use std::path::Path;

use crate::export;
use crate::types::{Config, Mode, TicketResult};

/// Process a single ticket JSON file through the full pipeline.
pub fn process_ticket(path: &Path, config: &Config) -> Result<TicketResult, String> {
    // Step 0: Freshness check (update mode)
    if matches!(config.mode, Mode::Update) {
        let (incident_number, opened_date) = load::preparse_ticket(path)?;
        let output_path = export::output_path(config, &incident_number, &opened_date);
        if is_up_to_date(path, &output_path)? {
            return Ok(TicketResult::UpToDate);
        }
    }

    // Step 1: Load
    let mut ticket = load::load_ticket(path, &config.input_dir)?;

    // Step 2: Ticket-level filter
    if filter::should_skip_ticket(&ticket.short_description) {
        return Ok(TicketResult::Filtered);
    }

    // Step 3: Normalize messages
    ticket.messages.iter_mut().for_each(|msg| {
        msg.text = normalize::clean_message_text(&msg.text, &msg.author);
    });

    // Filter out empty messages after normalization
    ticket.messages.retain(|msg| !msg.text.trim().is_empty());

    // Step 4: Deduplicate
    ticket.messages = dedup::deduplicate(ticket.messages);

    // Step 5: Post-extraction filters
    if ticket.messages.is_empty() {
        return Ok(TicketResult::Filtered);
    }
    if filter::all_bot_messages(&ticket.messages) {
        return Ok(TicketResult::Filtered);
    }
    if ticket.messages.len() == 1 && ticket.attachments.is_empty() {
        return Ok(TicketResult::Filtered);
    }

    // Step 6 & 7: Build timeline and export
    export::export_ticket(config, &mut ticket)?;

    Ok(TicketResult::Processed)
}

fn is_up_to_date(input: &Path, output: &Path) -> Result<bool, String> {
    if !output.exists() {
        return Ok(false);
    }

    let input_mtime = std::fs::metadata(input)
        .and_then(|m| m.modified())
        .map_err(|e| format!("{}: cannot read mtime: {}", input.display(), e))?;

    let output_mtime = std::fs::metadata(output)
        .and_then(|m| m.modified())
        .map_err(|e| format!("{}: cannot read mtime: {}", output.display(), e))?;

    Ok(output_mtime >= input_mtime)
}
