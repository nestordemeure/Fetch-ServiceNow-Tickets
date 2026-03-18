pub mod json;
pub mod markdown;

use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::types::{Config, OutputFormat, Ticket};

/// Compute the primary output file path for a ticket (used for freshness checks).
pub fn output_path(
    config: &Config,
    incident_number: &str,
    opened_date: &NaiveDate,
    json_input_path: &Path,
) -> PathBuf {
    match config.output_format {
        OutputFormat::Markdown => {
            markdown::output_path(&config.output_dir, incident_number, opened_date)
        }
        OutputFormat::Json => {
            json::output_path(json_input_path, &config.input_dir, &config.output_dir)
        }
    }
}

/// Export a ticket: resolve attachments, build timeline, render, and write to disk.
pub fn export_ticket(
    config: &Config,
    ticket: &mut Ticket,
    json_input_path: &Path,
) -> Result<(), String> {
    match config.output_format {
        OutputFormat::Markdown => markdown::export(config, ticket),
        OutputFormat::Json => {
            json::export(
                ticket,
                json_input_path,
                &config.input_dir,
                &config.output_dir,
                config.symlink_attachments,
            )
        }
    }
}
