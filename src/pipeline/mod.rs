pub mod attachments;
pub mod dedup;
pub mod filter;
pub mod load;
pub mod normalize;
pub mod timeline;

use std::path::Path;

use crate::export;
use crate::pii;
use crate::types::{Config, Mode, OutputFormat, PiiFilter, TicketResult};

/// Process a single ticket JSON file through the full pipeline.
pub fn process_ticket(path: &Path, config: &Config) -> Result<TicketResult, String> {
    // Step 0+1: Read JSON once, do freshness check, then load.
    // For JSON output, the output path is computable from the file path alone —
    // we can check freshness before even reading the file.
    if matches!(config.mode, Mode::Update) && matches!(config.output_format, OutputFormat::Json) {
        let output_path = export::json_output_path(path, config);
        if is_up_to_date(path, &output_path)? {
            return Ok(TicketResult::UpToDate);
        }
    }

    // Read and parse the JSON file once
    let data = load::read_json(path)?;

    // For markdown output in update mode, check freshness using parsed fields
    if matches!(config.mode, Mode::Update) && matches!(config.output_format, OutputFormat::Markdown) {
        let incident_number = load::extract_incident_number(&data, path)?;
        let opened_date = load::extract_opened_date(&data, path)?;
        let output_path = export::output_path(config, &incident_number, &opened_date, path);
        if is_up_to_date(path, &output_path)? {
            return Ok(TicketResult::UpToDate);
        }
    }

    // Load ticket from the already-parsed Value (no re-read)
    let mut ticket = load::load_ticket_from_value(data, path, &config.input_dir, &config.output_format)?;

    // Step 2a: Config-based filters (date, contact type, close code, state, creator, assignment group)
    if filter::should_skip_by_config(&ticket, &config.filter) {
        return Ok(TicketResult::Filtered);
    }

    // Step 2b: Ticket-level filter (short_description patterns)
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

    // Build PII matchers once for all PII operations
    let matchers = pii::build_pii_matchers(&ticket);

    // Step 5: PII filtering (message-level, skipped for JSON — recursive PII handles it)
    if !matches!(config.output_format, OutputFormat::Json) {
        pii::filter_pii(&mut ticket, &config.pii_filter, &matchers, config.deterministic_pii);
    }

    // Step 6: Post-extraction filters
    if ticket.messages.is_empty() {
        return Ok(TicketResult::Filtered);
    }
    if filter::all_bot_messages(&ticket.messages) {
        return Ok(TicketResult::Filtered);
    }
    if ticket.messages.len() == 1 && ticket.attachments.is_empty() {
        return Ok(TicketResult::Filtered);
    }

    // Determine PII context for attachment writing
    let pii_for_attachments = if !matches!(config.pii_filter, PiiFilter::None) {
        let deterministic = match config.output_format {
            OutputFormat::Json => true,
            OutputFormat::Markdown => config.deterministic_pii,
        };
        Some((&matchers, deterministic))
    } else {
        None
    };

    // Step 7: Build timeline and export
    export::export_ticket(config, &mut ticket, path, &matchers, pii_for_attachments)?;

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
