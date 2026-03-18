use std::sync::OnceLock;

use regex::Regex;

use crate::types::{FilterConfig, Message, Ticket};

/// Returns true if the ticket should be skipped based on its short_description.
pub fn should_skip_ticket(short_description: &Option<String>) -> bool {
    let desc = match short_description {
        Some(s) => s,
        None => return false,
    };

    is_iris_ticket(desc)
        || is_storage_quota_increase(desc)
        || is_compute_reservation_request(desc)
        || is_perlmutter_access_request(desc)
        || is_gpu_nodes_access_request(desc)
        || is_vasp_license_confirmation_request(desc)
        || is_collaboration_account_request(desc)
        || is_training_accounts_request(desc)
        || is_nersc_ip_request(desc)
        || is_nersc_cname_request(desc)
        || is_account_request_being_processed(desc)
        || is_account_in_new_allocation_year(desc)
        || is_account_reactivation_request(desc)
        || is_close_account_request(desc)
        || is_compute_reservation_request_variant(desc)
        || is_realtime_queue_access_request(desc)
        || is_node_hour_increase_request(desc)
        || is_travel_laptop_request(desc)
        || is_daily_rps_dynamic_screening_alert(desc)
        || is_slurm_iris_failure(desc)
        || is_high_load_warning(desc)
        || is_touching_scratch_warning(desc)
        || is_running_watch_warning(desc)
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

// Prefix match (case-insensitive): "Ticket from Iris:"
fn is_iris_ticket(desc: &str) -> bool {
    has_prefix_ignore_ascii_case(desc, "Ticket from Iris:")
}

// Prefix/exact match (case-insensitive): "Storage Quota Increase request:"
fn is_storage_quota_increase(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Storage Quota increase request")
        || has_prefix_ignore_ascii_case(desc, "Storage Quota Increase request:")
}

// Exact match (case-insensitive): "Compute Reservation Request"
fn is_compute_reservation_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Compute Reservation Request")
}

// Exact match (case-insensitive): "Perlmutter access request" / "Request perlmutter access"
fn is_perlmutter_access_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Perlmutter access request")
        || desc.eq_ignore_ascii_case("Request perlmutter access")
}

// Exact match (case-insensitive): "GPU nodes access request" / "Request access GPU nodes"
fn is_gpu_nodes_access_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("GPU nodes access request")
        || desc.eq_ignore_ascii_case("Request access GPU nodes")
}

// Exact match (case-insensitive): VASP license confirmation workflow requests
fn is_vasp_license_confirmation_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("VASP license Confirmation Request to Access NERSC Provided VASP Binaries")
}

// Exact match (case-insensitive): collaboration account request workflows
fn is_collaboration_account_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Collaboration account request")
        || desc.eq_ignore_ascii_case("Request collaboration account")
        || desc.eq_ignore_ascii_case("Request a collaboration account")
}

// Exact match (case-insensitive): "Training Accounts Request"
fn is_training_accounts_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Training Accounts Request")
}

// Exact match (case-insensitive): "NERSC IP REQUEST"
fn is_nersc_ip_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("NERSC IP REQUEST")
}

// Exact match (case-insensitive): "NERSC CNAME REQUEST"
fn is_nersc_cname_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("NERSC CNAME REQUEST")
}

// Exact match (case-insensitive): account onboarding/vetting status workflow
fn is_account_request_being_processed(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Re: Your NERSC account request is being processed")
}

// Exact match (case-insensitive): new allocation year account-status workflow
fn is_account_in_new_allocation_year(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Re: Your NERSC account in the new allocation year")
        || desc.eq_ignore_ascii_case("RE: Your NERSC account in the new allocation year")
}

// Exact match (case-insensitive): account reactivation workflows
fn is_account_reactivation_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Account Reactivation")
        || desc.eq_ignore_ascii_case("account reactivation")
        || desc.eq_ignore_ascii_case("reactivation account")
        || desc.eq_ignore_ascii_case("reactivate account")
        || desc.eq_ignore_ascii_case("Reactivate my account")
        || desc.eq_ignore_ascii_case("please reactivate my account")
        || desc.eq_ignore_ascii_case("Reactivating NERSC account")
        || desc.eq_ignore_ascii_case("reactivating my nersc account")
        || desc.eq_ignore_ascii_case("reactivating my account")
        || desc.eq_ignore_ascii_case("Reactivation of account")
        || desc.eq_ignore_ascii_case("Reactivation of Account")
        || desc.eq_ignore_ascii_case("Reactivate Account")
        || desc.eq_ignore_ascii_case("Reactivate account")
        || desc.eq_ignore_ascii_case("reactivate account?")
        || desc.eq_ignore_ascii_case("Reactivating account?")
}

// Exact match (case-insensitive): account closure workflows
fn is_close_account_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("close account")
        || desc.eq_ignore_ascii_case("Close account")
        || desc.eq_ignore_ascii_case("Close an account")
        || desc.eq_ignore_ascii_case("Close my account please")
        || desc.eq_ignore_ascii_case("Close the account")
        || desc.eq_ignore_ascii_case("Account Close")
        || desc.eq_ignore_ascii_case("account closing")
        || desc.eq_ignore_ascii_case("Account closing")
        || desc.eq_ignore_ascii_case("closing account")
        || desc.eq_ignore_ascii_case("Closing account")
}

// Exact match (case-insensitive): "Compute ReservationRequest"
fn is_compute_reservation_request_variant(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Compute ReservationRequest")
}

// Exact match (case-insensitive): "Realtime Queue Access Request"
fn is_realtime_queue_access_request(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("Realtime Queue Access Request")
}

// Prefix match (case-insensitive): CPU/GPU node-hour increase request workflow subjects
fn is_node_hour_increase_request(desc: &str) -> bool {
    has_prefix_ignore_ascii_case(desc, "CPU Node hour increase request for project ")
        || has_prefix_ignore_ascii_case(desc, "GPU Node hour increase request for project ")
}

// Substring match (case-insensitive): travel laptop admin workflows
fn is_travel_laptop_request(desc: &str) -> bool {
    desc.to_ascii_lowercase().contains("travel laptop")
}

// Prefix match (case-insensitive): "Daily RPS Dynamic Screening Alert ..."
fn is_daily_rps_dynamic_screening_alert(desc: &str) -> bool {
    has_prefix_ignore_ascii_case(desc, "Daily RPS Dynamic Screening Alert")
}

// Prefix match (case-insensitive): "Failure to run slurm_iris.py on ..."
fn is_slurm_iris_failure(desc: &str) -> bool {
    has_prefix_ignore_ascii_case(desc, "Failure to run slurm_iris.py on ")
}

// Prefix match (case-insensitive): "[response required] high load on ..."
fn is_high_load_warning(desc: &str) -> bool {
    has_prefix_ignore_ascii_case(desc, "[response required] high load on ")
}

// Exact match (case-insensitive): scratch touch policy warning
fn is_touching_scratch_warning(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("[response required] touching files in your scratch directory")
}

// Exact/prefix match (case-insensitive): watch-on-login-node policy warnings
fn is_running_watch_warning(desc: &str) -> bool {
    desc.eq_ignore_ascii_case("[response required] running watch on NERSC systems")
        || has_prefix_ignore_ascii_case(desc, "[response required] ")
            && desc.to_ascii_lowercase().contains(" running watch on nersc systems")
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

fn has_prefix_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len()
        && s.get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}
