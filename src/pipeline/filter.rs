use std::sync::OnceLock;

use regex::Regex;

use crate::types::{FilterConfig, Message, Ticket};

/// Returns true if the ticket should be skipped based on its short_description.
pub fn should_skip_ticket(short_description: &Option<String>) -> bool {
    let desc = match short_description {
        Some(s) => s,
        None => return false,
    };

    is_iris_pi_request(desc)
        || is_storage_quota_increase(desc)
        || is_training_renewal(desc)
        || is_training_expiring(desc)
        || is_nersc_account_activation(desc)
}

/// Returns true if all messages are from bot/system authors only.
pub fn all_bot_messages(messages: &[Message]) -> bool {
    !messages.is_empty()
        && messages.iter().all(|m| {
            let a = m.author.to_lowercase();
            a == "autoticketing" || a == "pm-node-info-bot" || a == "system"
        })
}

/// Returns true if the ticket should be skipped based on configurable filter rules.
pub fn should_skip_by_config(ticket: &Ticket, filter: &FilterConfig) -> bool {
    // 1. min_created_date: skip if ticket opened before this date
    if let Some(min_date) = filter.min_created_date {
        if ticket.opened_date < min_date {
            return true;
        }
    }

    // 2. exclude_contact_types: skip if contact_type (lowercased) is in the set
    if !filter.exclude_contact_types.is_empty() {
        if let Some(ct) = &ticket.contact_type {
            if filter
                .exclude_contact_types
                .iter()
                .any(|exc| exc == &ct.to_lowercase())
            {
                return true;
            }
        }
    }

    // 3. include_close_codes: if non-empty, skip if close_code is NOT in the set
    if !filter.include_close_codes.is_empty() {
        match &ticket.close_code {
            Some(cc) => {
                if !filter.include_close_codes.iter().any(|inc| inc == cc) {
                    return true;
                }
            }
            None => return true,
        }
    }

    // 4. require_closed_or_resolved: skip if status is not "closed" or "resolved"
    if filter.require_closed_or_resolved {
        let status_lower = ticket.status.to_lowercase();
        if status_lower != "closed" && status_lower != "resolved" {
            return true;
        }
    }

    // 5. exclude_created_by: skip if regex matches created_by
    if let Some(re) = &filter.exclude_created_by {
        if let Some(cb) = &ticket.created_by {
            if re.is_match(cb) {
                return true;
            }
        }
    }

    // 6. exclude_assignment_group: skip if regex matches assignment_group
    if let Some(re) = &filter.exclude_assignment_group {
        if let Some(ag) = &ticket.assignment_group {
            if re.is_match(ag) {
                return true;
            }
        }
    }

    false
}

// Exact match (case-insensitive): "Ticket from Iris: New PI Account Request"
fn is_iris_pi_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Ticket from Iris: New PI Account Request")
}

// Prefix match (case-insensitive): "Storage Quota Increase request:"
fn is_storage_quota_increase(desc: &str) -> bool {
    let prefix = "Storage Quota Increase request:";
    desc.len() >= prefix.len()
        && desc.get(..prefix.len()).is_some_and(|s| s.eq_ignore_ascii_case(prefix))
}

// Regex match (case-insensitive): ^Renewal of .+ Training for Staff$
fn is_training_renewal(desc: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)^Renewal of .+ Training for Staff$").unwrap()
    });
    re.is_match(desc)
}

// Substring match (case-insensitive): contains "Training expiring"
fn is_training_expiring(desc: &str) -> bool {
    desc.to_ascii_lowercase().contains("training expiring")
}

// Substring match (case-insensitive): contains "NERSC Account activation"
fn is_nersc_account_activation(desc: &str) -> bool {
    let lower = desc.to_ascii_lowercase();
    lower.contains("nersc account activation")
}
