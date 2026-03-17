use std::sync::OnceLock;

use regex::Regex;

use crate::types::Message;

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

/// Returns true if all messages are from bot authors only.
pub fn all_bot_messages(messages: &[Message]) -> bool {
    !messages.is_empty()
        && messages
            .iter()
            .all(|m| m.author == "autoticketing" || m.author == "pm-node-info-bot")
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
