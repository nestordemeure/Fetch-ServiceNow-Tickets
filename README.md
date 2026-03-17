# ServiceNow Ticket Processor

Processes NERSC ServiceNow incident exports (JSON) into clean, normalized output files suitable for AI coding assistants.
Tickets are filtered, their messages are cleaned of boilerplate (greetings, footers, signoffs, metadata), deduplicated, and exported as structured markdown organized by date and incident number.
The processing pipeline runs in parallel across all available CPU cores.

## Usage

Edit `config.toml` to set your input directory and preferences:

```toml
input_dir = "/path/to/servicenow/json/exports"
output_dir = "./tickets"
output_format = "markdown"
mode = "update"
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

All tickets are discovered and processed in parallel using Rayon:

0. **Freshness check**: in `update` mode, compare the input JSON's modification time against its output file. Skip the ticket entirely if the output is already up-to-date. In `replace` mode, all tickets are processed unconditionally.

1. **Load**: read the JSON file and deserialize it. Missing or unparseable required fields cause a hard error naming the file and field.

2. **Filter (config-based)**: skip the ticket based on configurable rules in the `[filter]` section:
   - `min_created_date`: exclude tickets opened before a given date.
   - `exclude_contact_types`: exclude tickets with specific contact types (e.g. "Staff Initiated", "System Alert Auto Generated").
   - `include_close_codes`: only keep tickets with specific close codes (empty = accept all).
   - `require_closed_or_resolved`: only keep tickets whose state is "Closed" or "Resolved".
   - `exclude_created_by`: exclude tickets whose creator matches a regex.
   - `exclude_assignment_group`: exclude tickets whose assignment group matches a regex.

3. **Filter (short_description)**: skip the ticket if its `short_description` matches any of:
   - Exact match: `"Ticket from Iris: New PI Account Request"`.
   - Prefix: `"Storage Quota Increase request:"`.
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

6. **PII filtering** (controlled by `pii_filter` and `deterministic_pii`): redact personally identifiable information from message text. Names and usernames (extracted from ticket metadata) are replaced with `[NAME]` via Aho-Corasick dictionary matching. Emails are replaced with `[EMAIL]`, phone numbers with `[PHONE]`, and password values with `[PASSWORD]`. Username-in-context patterns (shell logins like `user@host`, NERSC home paths like `/global/homes/u/username`, and command flags like `-u username`) are also detected and redacted. Three PII modes: `"all"` filters every message, `"asker"` filters only the original ticket opener's messages, `"none"` disables filtering. When `deterministic_pii = true`, names and emails are replaced with HMAC-based pseudonyms (e.g. `USER_A3F2B1C9D0`, `EMAIL_B4E8C2A1F7`) instead of generic placeholders, preserving identity linkage across tickets.

7. **Filter (post-extraction)**: skip the ticket if: zero messages remain, all messages are from bots (`autoticketing`, `pm-node-info-bot`, `system`), or exactly one message with no attachments.

8. **Build timeline**: merge messages and attachments into chronological order (sorted by timestamp, messages before attachments at equal timestamps, consecutive attachments grouped). Copy attachment files from `local_path` (resolved relative to `input_dir`), sanitizing filenames for uniqueness.

9. **Export**: render the timeline in the configured output format (currently markdown) and write to `<output_dir>/YYYY/MM/INC########/ticket.md` alongside any attachment files.

## TODO

- Add Athos output data format
- Import tickets directly from the ServiceNow API
  - Add scron script to refresh tickets regularly
