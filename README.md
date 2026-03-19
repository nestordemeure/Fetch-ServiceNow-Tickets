# ServiceNow Ticket Processor

Processes NERSC ServiceNow incident exports (JSON) into clean, normalized output files suitable for AI coding assistants. Tickets are filtered, their messages cleaned of boilerplate, deduplicated, and exported as structured markdown or sanitized JSON. Processing runs in parallel across all available CPU cores.

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

JSON files are discovered recursively via `walkdir` (using `d_type` to skip `stat()` on networked filesystems) and processed in parallel via Rayon. Each ticket passes through these stages:

1. **Freshness check** (`update` mode): skip if the output file is newer than the input JSON. For JSON output, the output path is computed from the file path alone. For markdown, the JSON is parsed once and reused.

2. **Load**: deserialize JSON into internal types. Missing required fields cause a hard error naming the file and field.

3. **Filter**: skip tickets based on:
   - Config rules: date range, contact type, close code, state, creator regex, assignment group regex.
   - Short-description patterns: ~30 regex patterns matching workflow/administrative ticket subjects (Iris, storage quota, ERCAP, compute reservations, access requests, account lifecycle, allocation notices, training, screening alerts, etc.). See [SPECIFICATIONS.md §4.2.2](docs/SPECIFICATIONS.md) for the full list.
   - Post-extraction: skip tickets with zero messages, all-bot messages, or exactly one message with no attachments.

4. **Normalize** messages: strip metadata headers, greetings, NERSC footers, signoffs, signature blocks, and author name lines. Trim whitespace. See [SPECIFICATIONS.md §4.3](docs/SPECIFICATIONS.md) for details.

5. **Deduplicate**: remove consecutive messages with identical text and same visibility.

6. **PII redaction** (configurable: `all` / `asker` / `none`): redact names, usernames, emails, phones, passwords, Zoom meeting details, shell logins, NERSC paths, and command user flags. The ticket opener's name → `[ASKER]`, other names → `[NAME]`, login IDs → `[USERNAME]`. Optional deterministic HMAC pseudonyms (`USER_<HMAC>`, `EMAIL_<HMAC>`) replace all of these consistently.

7. **Build timeline**: merge messages and attachments chronologically. Consecutive attachments are grouped.

8. **Export**:
   - **Markdown**: `<output_dir>/YYYY/MM/INC########/ticket.md` with attachments alongside. When `symlink_attachments = true`, attachments are symlinked; otherwise copied. Text attachments are PII-scanned when PII is enabled.
   - **JSON**: preserves original JSON structure with cleaned discussions, recursive PII sanitization across all fields, sorted keys, 2-space indent. Attachments preserve input-relative paths.

See [docs/SPECIFICATIONS.md](docs/SPECIFICATIONS.md) for the full pipeline specification and [docs/ticket_format_specification.md](docs/ticket_format_specification.md) for the markdown output format.

## TODO

- update claude.md
  - include readme and docs/specs
  - require clippy runs and fixes after major code changes
  - introduce agent.md, the claude file just loading it

- in generated agent.md, have path be absolute

- Import tickets directly from the ServiceNow API
  - Add scron script to refresh tickets regularly
