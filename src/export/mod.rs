pub mod markdown;

use std::path::PathBuf;

use chrono::NaiveDate;

use crate::types::{Config, OutputFormat, Ticket};

/// Compute the primary output file path for a ticket (used for freshness checks).
pub fn output_path(config: &Config, incident_number: &str, opened_date: &NaiveDate) -> PathBuf {
    match config.output_format {
        OutputFormat::Markdown => markdown::output_path(&config.output_dir, incident_number, opened_date),
    }
}

/// Export a ticket: resolve attachments, build timeline, render, and write to disk.
pub fn export_ticket(config: &Config, ticket: &mut Ticket) -> Result<(), String> {
    match config.output_format {
        OutputFormat::Markdown => markdown::export(config, ticket),
    }
}
