# ServiceNow Ticket Processor

Processes NERSC ServiceNow incident exports (JSON) into clean, normalized output files suitable for AI coding assistants.
Tickets are filtered, their messages are cleaned of boilerplate (greetings, footers, signoffs, metadata), deduplicated, and exported as structured markdown organized by date and incident number.
The processing pipeline runs in parallel across all available CPU cores.

## Usage

Edit `config.toml` to set your input directory and preferences:

```toml
input_dir = "/path/to/servicenow/json/exports"
output_dir = "./tickets"
output_format = "markdown"  # or "json"
mode = "update"
symlink_attachments = true
pii_filter = "all"
deterministic_pii = false

[filter]
min_created_date = ""
exclude_contact_types = ["Staff Initiated", "System Alert Auto Generated"]
include_close_codes = []
require_closed_or_resolved = false
exclude_created_by = ""
exclude_assignment_group = ""
```

Then build and run:

```sh
module load rust # on NERSC systems
cargo run --release
```

## How It Works

Ticket JSON files are discovered by `walkdir` (using `d_type` to avoid `stat()` overhead on networked filesystems) and processed in parallel via Rayon:

0. **Freshness check**: in `update` mode, compare the input JSON's modification time against its output file. For JSON output, the output path is computed from the file path alone (no parsing needed). For markdown output, the JSON is read once and reused for the full pipeline. Skip the ticket entirely if the output is already up-to-date. In `replace` mode, all tickets are processed unconditionally.

1. **Load**: read the JSON file and deserialize it (reusing the parse from step 0 if already done). Missing or unparseable required fields cause a hard error naming the file and field.

2. **Filter (config-based)**: skip the ticket based on configurable rules in the `[filter]` section:
   - `min_created_date`: exclude tickets opened before a given date.
   - `exclude_contact_types`: exclude tickets with specific contact types (e.g. "Staff Initiated", "System Alert Auto Generated").
   - `include_close_codes`: only keep tickets with specific close codes (empty = accept all).
   - `require_closed_or_resolved`: only keep tickets whose state is "Closed" or "Resolved".
   - `exclude_created_by`: exclude tickets whose creator matches a regex.
   - `exclude_assignment_group`: exclude tickets whose assignment group matches a regex.

3. **Filter (short_description)**: skip the ticket if its `short_description` matches any of:
   - Prefix: `"Ticket from Iris:"`.
   - Exact match: `"Storage Quota increase request"` or prefix `"Storage Quota Increase request:"`.
   - Exact match: `"Compute Reservation Request"`.
   - Exact match: `"Perlmutter access request"` or `"Request perlmutter access"`.
   - Exact match: `"GPU nodes access request"` or `"Request access GPU nodes"`.
   - Exact match: `"VASP license Confirmation Request to Access NERSC Provided VASP Binaries"`.
   - Exact match: `"Collaboration account request"`, `"Request collaboration account"`, or `"Request a collaboration account"`.
   - Exact match: `"Training Accounts Request"`.
   - Exact match: `"NERSC IP REQUEST"`.
   - Exact match: `"NERSC CNAME REQUEST"`.
   - Exact match: `"Re: Your NERSC account request is being processed"`.
   - Exact match: `"Re: Your NERSC account in the new allocation year"`.
   - Account reactivation variants such as `"Account Reactivation"` and `"reactivate account"`.
   - Account closure variants such as `"close account"` and `"closing account"`.
   - Exact match: `"Compute ReservationRequest"`.
   - Exact match: `"Realtime Queue Access Request"`.
   - Prefix: `"CPU Node hour increase request for project "` or `"GPU Node hour increase request for project "`.
   - Substring: `"travel laptop"`.
   - Prefix: `"Daily RPS Dynamic Screening Alert"`.
   - Prefix: `"Failure to run slurm_iris.py on "`.
   - Prefix: `"[response required] high load on "`.
   - Exact match: `"[response required] touching files in your scratch directory"`.
   - Exact match: `"[response required] running watch on NERSC systems"` or subject variants ending in `"running watch on NERSC systems"`.
   - Regex: `^Renewal of .+ Training for Staff$`.
   - Substring: `"Training expiring"`.
   - Substring: `"NERSC Account activation"`.

4. **Extract and normalize messages**: pull messages from `customer_facing_comments` and `internal_work_notes`. Skip messages authored by `"System"`. Strip the `(staff work notes (NERSC private))` suffix from internal author names. Clean each message's text in order:
   - Strip leading metadata lines (`reply from:`, `created by:`, etc.).
   - Remove a greeting line if first (`Hi`, `Hello`, `Dear <name>`, `Good morning`, etc.).
   - Remove a trailing standalone date or email quote header.
   - Remove NERSC footer lines (`NERSC Consulting`, `accounts@nersc.gov`, etc.).
   - Remove closing signoff (`Best`, `Regards`, `Thanks`, etc.) and optional trailing name.
   - Remove lines matching the author's name.
   - Trim leading/trailing blank lines.
   - Discard messages that are empty after cleaning.

5. **Deduplicate**: remove consecutive messages with identical text and same visibility (internal/customer-facing), keeping the first.

6. **PII filtering** (controlled by `pii_filter` and `deterministic_pii`): redact personally identifiable information from message text. Names and usernames (extracted from ticket metadata) are replaced with `[NAME]` via Aho-Corasick dictionary matching. Emails are replaced with `[EMAIL]`, phone numbers with `[PHONE]`, and password values with `[PASSWORD]`. Username-in-context patterns (shell logins like `user@host`, NERSC home paths like `/global/homes/u/username`, and command flags like `-u username`) are also detected and redacted. Three PII modes: `"all"` filters every message, `"asker"` filters only the original ticket opener's messages, `"none"` disables filtering. When `deterministic_pii = true`, names and emails are replaced with HMAC-based pseudonyms (e.g. `USER_A3F2B1C9D0`, `EMAIL_B4E8C2A1F7`) instead of generic placeholders, preserving identity linkage across tickets. **Note:** for JSON output, message-level PII is skipped — the recursive JSON PII step (see below) handles all strings including message text.

7. **Filter (post-extraction)**: skip the ticket if: zero messages remain, all messages are from bots (`autoticketing`, `pm-node-info-bot`, `system`), or exactly one message with no attachments.

8. **Build timeline**: merge messages and attachments into chronological order (sorted by timestamp, messages before attachments at equal timestamps, consecutive attachments grouped). Attachment filenames are sanitized for uniqueness before export.

9. **Export**: render the timeline in the configured output format and write to disk:
   - **Markdown**: write `<output_dir>/YYYY/MM/INC########/ticket.md`, then write any attachment outputs alongside it. When `symlink_attachments = true`, those outputs are symbolic links to the source files; when `false`, they are copied files. When PII filtering is enabled, text attachments are scanned for PII and written with redacted content if any is found (overriding symlink mode for those files). Binary and non-UTF-8 files are always copied/symlinked without modification. If an attachment write fails, the markdown file is left in place as partial output.
   - **JSON**: write processed messages back into the original JSON structure, apply recursive PII sanitization to the entire JSON tree (structured user fields → `USER_<HMAC>`, email fields → `EMAIL_<HMAC>`, watch-list fields → comma-separated aliases, all other strings → free-text scan for emails, shell logins, NERSC paths, command flags, phones, passwords, and names), serialize with sorted keys and 2-space indentation, and write to `<output_dir>/<relative_input_path>`. Attachment outputs preserve their relative paths from the input directory and are symlinked or copied according to `symlink_attachments`. Text attachments are PII-scanned when PII filtering is enabled (always deterministic for JSON).

## TODO

- tickets type worth filtering out:
  - "Storage Quota increase - extension"
  - "ERCAP request"
  - "Re: [Users] ERCAP Requests due by 11:59 pm; Join us for ERCAP Office Hours!"

- run clippy over everything
- anonimization in message authors makes the format harder to read
- review all texts (readme, claude, specs)
- Import tickets directly from the ServiceNow API
  - Add scron script to refresh tickets regularly
