use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;
use std::path::PathBuf;

pub struct Config {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub output_format: OutputFormat,
    pub mode: Mode,
    pub pii_filter: PiiFilter,
    pub deterministic_pii: bool,
    pub filter: FilterConfig,
}

pub struct FilterConfig {
    pub min_created_date: Option<NaiveDate>,
    pub exclude_contact_types: Vec<String>, // lowercased at parse time
    pub include_close_codes: Vec<String>,
    pub require_closed_or_resolved: bool,
    pub exclude_created_by: Option<Regex>,
    pub exclude_assignment_group: Option<Regex>,
}

pub enum PiiFilter {
    /// Filter PII from all messages.
    All,
    /// Filter PII only from the original ticket opener's messages.
    Asker,
    /// No PII filtering.
    None,
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
    pub contact_type: Option<String>,
    pub close_code: Option<String>,
    pub created_by: Option<String>,
    pub assignment_group: Option<String>,
    pub messages: Vec<Message>,
    pub attachments: Vec<Attachment>,
    /// Names, usernames, and IDs extracted from ticket data for PII filtering.
    pub known_pii: Vec<String>,
    /// Author of the first customer-facing message (the original asker).
    pub opener: Option<String>,
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
