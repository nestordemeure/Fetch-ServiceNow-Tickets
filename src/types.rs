use chrono::{NaiveDate, NaiveDateTime};
use std::path::PathBuf;

pub struct Config {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub output_format: OutputFormat,
    pub mode: Mode,
}

pub enum OutputFormat {
    Markdown,
}

pub enum Mode {
    Update,
    Replace,
}

pub struct Ticket {
    pub incident_number: String,
    pub short_description: Option<String>,
    pub status: String,
    pub opened_date: NaiveDate,
    pub closed_date: Option<NaiveDate>,
    pub messages: Vec<Message>,
    pub attachments: Vec<Attachment>,
}

pub struct Message {
    pub author: String,
    pub timestamp: NaiveDateTime,
    pub text: String,
    pub internal: bool,
}

pub struct Attachment {
    pub original_name: String,
    pub resolved_name: String,
    pub timestamp: NaiveDateTime,
    pub local_path: PathBuf,
}

pub enum TimelineEntryKind {
    Message {
        author: String,
        text: String,
        internal: bool,
    },
    AttachmentGroup(Vec<String>),
}

pub struct TimelineEntry {
    pub timestamp: NaiveDateTime,
    pub kind: TimelineEntryKind,
    pub order: usize,
}

/// Result of processing a single ticket file.
pub enum TicketResult {
    Processed,
    Filtered,
    UpToDate,
}
