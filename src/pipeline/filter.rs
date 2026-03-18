use std::sync::OnceLock;

use regex::RegexSet;

use crate::types::{FilterConfig, Message, Ticket};

/// Lazily-compiled RegexSet for all short_description skip patterns.
/// All patterns are case-insensitive. Compiled once, matched in a single pass.
fn skip_patterns() -> &'static RegexSet {
    static SET: OnceLock<RegexSet> = OnceLock::new();
    SET.get_or_init(|| {
        RegexSet::new([
            // is_iris_ticket — prefix
            r"(?i)^Ticket from Iris:",
            // is_storage_quota_increase — workflow subjects, including request and extension variants
            r"(?i)^Storage Quota increase(?: request)?(?:\s*[:\-].*|$)",
            // is_compute_reservation_request + variant — exact (optional space)
            r"(?i)^Compute Reservation\s?Request$",
            // is_perlmutter_access_request — exact
            r"(?i)^(Perlmutter access request|Request perlmutter access)$",
            // is_gpu_nodes_access_request — exact
            r"(?i)^(GPU nodes access request|Request access GPU nodes)$",
            // is_vasp_license_confirmation_request — exact
            r"(?i)^VASP license Confirmation Request to Access NERSC Provided VASP Binaries$",
            // is_collaboration_account_request — exact
            r"(?i)^(Collaboration account request|Request (a )?collaboration account)$",
            // is_training_accounts_request — exact
            r"(?i)^Training Accounts Request$",
            // is_nersc_ip_request — exact
            r"(?i)^NERSC IP REQUEST$",
            // is_nersc_cname_request — exact
            r"(?i)^NERSC CNAME REQUEST$",
            // is_account_request_being_processed — exact
            r"(?i)^Re: Your NERSC account request is being processed$",
            // is_account_in_new_allocation_year — exact
            r"(?i)^Re: Your NERSC account in the new allocation year$",
            // is_account_reactivation_request — exact variants
            r"(?i)^(account reactivation|reactivat(e|ion( of)?|ing) (my )?(nersc )?account\??|please reactivate my account)$",
            // is_close_account_request — exact variants
            r"(?i)^(close (an |my |the )?account( please)?|account clos(e|ing)|closing account)$",
            // is_ercap_request — workflow subjects and mailing-list announcement replies
            r"(?i)^ERCAP request(s)?(?:\b|$)",
            r"(?i)^Re:\s*\[Users\]\s*ERCAP\b",
            // is_realtime_queue_access_request — exact
            r"(?i)^Realtime Queue Access Request$",
            // is_node_hour_increase_request — prefix
            r"(?i)^(CPU|GPU) Node hour increase request for project ",
            // is_travel_laptop_request — substring
            r"(?i)travel laptop",
            // is_daily_rps_dynamic_screening_alert — prefix
            r"(?i)^Daily RPS Dynamic Screening Alert",
            // is_slurm_iris_failure — prefix
            r"(?i)^Failure to run slurm_iris\.py on ",
            // is_high_load_warning — prefix
            r"(?i)^\[response required\] high load on ",
            // is_touching_scratch_warning — exact
            r"(?i)^\[response required\] touching files in your scratch directory$",
            // is_running_watch_warning — prefix + contains
            r"(?i)^\[response required\] .*running watch on nersc systems",
            // is_training_renewal — regex
            r"(?i)^Renewal of .+ Training for Staff$",
            // is_training_expiring — substring
            r"(?i)training expiring",
            // is_nersc_account_activation — substring
            r"(?i)nersc account activation",
        ])
        .expect("skip_patterns: invalid regex")
    })
}

/// Returns true if the ticket should be skipped based on its short_description.
pub fn should_skip_ticket(short_description: &Option<String>) -> bool {
    let desc = match short_description {
        Some(s) => s,
        None => return false,
    };
    skip_patterns().is_match(desc)
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

#[cfg(test)]
mod tests {
    use super::should_skip_ticket;

    #[test]
    fn skips_storage_quota_extension_subjects() {
        assert!(should_skip_ticket(&Some(
            "Storage Quota increase - extension".to_string()
        )));
        assert!(should_skip_ticket(&Some(
            "Storage Quota increase request: project m1234".to_string()
        )));
    }

    #[test]
    fn skips_ercap_request_subjects() {
        assert!(should_skip_ticket(&Some("ERCAP request".to_string())));
        assert!(should_skip_ticket(&Some("ERCAP Requests".to_string())));
        assert!(should_skip_ticket(&Some(
            "Re: [Users] ERCAP Requests due by 11:59 pm; Join us for ERCAP Office Hours!"
                .to_string()
        )));
    }

    #[test]
    fn does_not_skip_unrelated_subjects() {
        assert!(!should_skip_ticket(&Some(
            "Question about ERCAP award data export".to_string()
        )));
        assert!(!should_skip_ticket(&Some(
            "Storage system performance issue".to_string()
        )));
    }
}
