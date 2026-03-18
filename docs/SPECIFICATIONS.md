# Specifications: Rust Rewrite of ServiceNow Ticket Processor

This document specifies the Rust implementation that replaces the Python `format_tickets.py` script. The goal is a parallel, high-throughput ticket processing pipeline that reads ServiceNow incident JSON exports and produces clean, normalized output files.

## 1. Configuration

A TOML configuration file (`config.toml`) at the project root controls all runtime parameters.

```toml
# Path to the directory containing raw ServiceNow incident JSON files.
input_dir = "/home/nestor/Documents/work/NERSC/tickets/demo_ticket_stash/pengfei"

# Path to the directory where processed tickets are written.
output_dir = "./tickets"

# Output format: "markdown" or "json".
output_format = "markdown"

# Update mode:
#   "replace" - wipe and rebuild the entire output directory.
#   "update"  - only re-process tickets whose source JSON is newer
#               than the corresponding output file (or has no output yet).
mode = "update"

# Attachment handling:
#   true  - write symbolic links into the output tree
#   false - copy attachment files into the output tree
symlink_attachments = true

# PII filtering:
#   "all"   - filter PII from all messages (staff and asker).
#   "asker" - filter PII only from the original ticket opener's messages.
#   "none"  - no PII filtering.
pii_filter = "all"

# When true, PII replacements use deterministic HMAC-based aliases
# (e.g. USER_A3F2B1C9D0, EMAIL_B4E8C2A1F7) instead of generic placeholders.
# Same input always produces the same alias, preserving identity linkage.
deterministic_pii = false

[filter]
min_created_date = ""
exclude_contact_types = ["Staff Initiated", "System Alert Auto Generated"]
include_close_codes = []
require_closed_or_resolved = false
exclude_created_by = ""
exclude_assignment_group = ""
```

All fields are **required** (including all fields in the `[filter]` section). If any field is missing, the program exits with an error message naming the missing field and listing the valid values (e.g. `output_format` accepts `"markdown"` or `"json"`, `mode` accepts `"update"` or `"replace"`). Empty strings and empty arrays are valid values for filter fields that should have no effect.

## 2. Project Structure

```
src/
  main.rs              - entry point: loads config, orchestrates the pipeline
  config.rs            - TOML config parsing and validation
  types.rs             - shared data structures (Ticket, Message, Attachment, TimelineEntry)
  pii/
    mod.rs             - PII public API: build_name_matcher(), filter_pii()
    redact.rs          - string-level PII: regexes, hmac_tag(), redact_text(), all helpers
    json.rs            - recursive JSON tree PII sanitization (athos-compatible)
    attachments.rs     - text file detection + PII redaction for attachment files
  pipeline/
    mod.rs             - pipeline module, per-ticket processing orchestration
    load.rs            - JSON deserialization of a ticket file into model types
    filter.rs          - ticket-level and message-level filtering rules
    normalize.rs       - message text cleaning (metadata, greetings, footers, signoffs, etc.)
    dedup.rs           - consecutive duplicate message removal
    timeline.rs        - merge messages and attachments into a sorted timeline
    attachments.rs     - attachment extraction, filename sanitization, PII-aware writing
  export/
    mod.rs             - output format dispatch
    markdown.rs        - markdown rendering of a processed ticket
    json.rs            - JSON export with write-back, recursive PII, and sorted-key serialization
config.toml            - default configuration file
```

## 3. Data Model

### 3.1 Input JSON Structure

Each JSON file represents one incident and has this shape:

```json
{
  "metadata": { "incident_number": "INC0228579" },
  "incident_fields": {
    "number": "INC0228579",
    "short_description": "...",
    "state": "Closed",
    "opened_at": "2025-01-15 14:22:00",
    "closed_at": "2025-01-16 09:15:00"
  },
  "discussions": {
    "customer_facing_comments": [
      { "created_by": "Jane Smith", "timestamp": "2025-01-15 14:30:00", "text": "..." }
    ],
    "internal_work_notes": [
      { "created_by": "John Doe (staff work notes (NERSC private))", "timestamp": "2025-01-15 15:00:00", "text": "..." }
    ]
  },
  "attachments": [
    {
      "file_name": { "display_value": "logs.tar.gz", "value": "logs.tar.gz" },
      "sys_created_on": { "display_value": "2025-01-15 14:35:00", "value": "2025-01-15 22:35:00" },
      "local_path": "servicenow_incidents/INC022/85/37_20260216_031410_attachements/logs.tar.gz"
    }
  ]
}
```

**No fallback key chains.** Fields are read from their exact expected keys. The input is trusted and well-formed. Missing or unparseable required fields cause a hard error identifying the file path and the field that failed — this is a data problem that must be surfaced, not papered over.

Expected keys per context:
- **Messages**: `timestamp`, `created_by`, `text` — plain strings.
- **Attachments**: fields are wrapped in `{"display_value": "...", "value": "..."}` objects. Use the `value` field. Expected keys: `file_name`, `sys_created_on` (for timestamp). Content is loaded from `local_path` (a path relative to the input root directory). The attachment has no inline base64 — files are always on disk.
- **Incident fields**: `number`, `state`, `opened_at`, `short_description` — plain strings. `closed_at`, `contact_type`, `close_code`, `sys_created_by`, `assignment_group` are optional (used for configurable filtering).

### 3.2 Internal Types

Timestamps are parsed once at load time into a single datetime type (e.g. `chrono::NaiveDateTime` or equivalent). No raw strings are kept — the parsed value is used for sorting, comparison, and formatting on output.

```
Ticket {
  incident_number: String,
  short_description: Option<String>,
  status: String,
  opened_date: NaiveDate,
  closed_date: Option<NaiveDate>,
  contact_type: Option<String>,
  close_code: Option<String>,
  created_by: Option<String>,        // sys_created_by
  assignment_group: Option<String>,
  messages: Vec<Message>,
  attachments: Vec<Attachment>,
  known_pii: Vec<String>,       // names, usernames extracted from ticket metadata
  opener: Option<String>,       // author of the first customer-facing message
  raw_json: Option<Value>,      // original JSON, preserved only for Json output format
}

Message {
  author: String,
  timestamp: NaiveDateTime,
  text: String,
  internal: bool,
  source_index: Option<usize>,  // position in original JSON array, for write-back in Json export
}

Attachment {
  original_name: String,
  resolved_name: String,      // after sanitization and dedup
  timestamp: NaiveDateTime,
  local_path: PathBuf,        // path to the file on disk, relative to input root
}

TimelineEntry {
  timestamp: NaiveDateTime,
  kind: Message | AttachmentGroup(Vec<String>),
  order: usize,               // original insertion order for stable sort
}
```

## 4. Pipeline

Each ticket JSON file goes through the following stages in order. The pipeline is applied to all files in parallel. The markdown output format is specified in [`ticket_format_specification.md`](./ticket_format_specification.md); this section is the canonical reference for all pipeline behavior.

### 4.1 Load (`pipeline/load.rs`)

- Read the JSON file from disk.
- Deserialize into the internal `Ticket` model. Each field is read from its expected key — no fallback chains.
- Parse timestamps into `NaiveDateTime`. Supported formats: `%Y-%m-%d %H:%M:%S` and `%Y-%m-%dT%H:%M:%S`.
- Extract the opened date from `opened_at`, closed date from `closed_at`. Both are truncated to `YYYY-MM-DD`. `closed_at` may be absent (ticket still open); all other required fields must be present or the ticket fails with an error identifying the file and field.

### 4.2 Filter (`pipeline/filter.rs`)

#### 4.2.1 Config-Based Filters

Skip the ticket based on configurable rules in the `[filter]` config section. Checked in order:

1. **`min_created_date`**: skip if `ticket.opened_date` is before this date. Empty string = no filter.
2. **`exclude_contact_types`**: skip if the ticket's `contact_type` (case-insensitive) matches any entry. Common values: `"Staff Initiated"`, `"System Alert Auto Generated"`.
3. **`include_close_codes`**: if non-empty, skip if the ticket's `close_code` is NOT in the set. Empty array = accept all.
4. **`require_closed_or_resolved`**: skip if the ticket's `state` (case-insensitive) is not `"Closed"` or `"Resolved"`.
5. **`exclude_created_by`**: skip if the ticket's `sys_created_by` matches this regex. Empty string = no filter.
6. **`exclude_assignment_group`**: skip if the ticket's `assignment_group` matches this regex. Empty string = no filter.

#### 4.2.2 Short-Description Filters

Skip the entire ticket if any of these match `short_description`:
1. **Iris workflow ticket**: prefix match (case-insensitive) `"Ticket from Iris:"`.
2. **Storage quota increase**: workflow-subject match (case-insensitive) for `"Storage Quota increase"` with optional `" request"` and optional `:` / `-` suffix content, covering cases like `"Storage Quota increase - extension"` and `"Storage Quota increase request: project ..."`.
3. **Compute reservation request**: exact match (case-insensitive) `"Compute Reservation Request"`.
4. **Perlmutter access request**: exact match (case-insensitive) `"Perlmutter access request"` or `"Request perlmutter access"`.
5. **GPU nodes access request**: exact match (case-insensitive) `"GPU nodes access request"` or `"Request access GPU nodes"`.
6. **VASP license confirmation request**: exact match (case-insensitive) `"VASP license Confirmation Request to Access NERSC Provided VASP Binaries"`.
7. **Collaboration account request**: exact match (case-insensitive) `"Collaboration account request"`, `"Request collaboration account"`, or `"Request a collaboration account"`.
8. **Training accounts request**: exact match (case-insensitive) `"Training Accounts Request"`.
9. **NERSC IP request**: exact match (case-insensitive) `"NERSC IP REQUEST"`.
10. **NERSC CNAME request**: exact match (case-insensitive) `"NERSC CNAME REQUEST"`.
11. **Account request being processed**: exact match (case-insensitive) `"Re: Your NERSC account request is being processed"`.
12. **Account in new allocation year**: exact match (case-insensitive) `"Re: Your NERSC account in the new allocation year"`.
13. **Account reactivation workflow**: exact-match variants such as `"Account Reactivation"`, `"reactivation account"`, `"reactivate account"`, `"Reactivate my account"`, and `"Reactivating NERSC account"`.
14. **Account closure workflow**: exact-match variants such as `"close account"`, `"Close my account please"`, `"Account Close"`, and `"closing account"`.
15. **Compute reservation request variant**: exact match (case-insensitive) `"Compute ReservationRequest"`.
16. **ERCAP workflow / announcement**: match (case-insensitive) subjects beginning with `"ERCAP request"` / `"ERCAP Requests"` or mailing-list reply subjects beginning with `"Re: [Users] ERCAP"`.
17. **ERCAP status notification**: match (case-insensitive) `Re:` / `Fw:` / `Fwd:` subjects, with optional `Update Closed Incident:` and optional `[EXTERNAL]`, of the form `"Your <year> NERSC ERCAP Request # ERCAP<number> has been denied"`, `"needs revision"`, or `"has been submitted for review"`.
18. **ERCAP deadline reminder**: match (case-insensitive) `Re:` / `Fw:` / `Fwd:` reminder mailers such as `"ATTENTION, ACTION NEEDED: AY <year> ERCAP renewal requests are due ..."`, `"Last chance to submit a renewal ERCAP request for AY <year>!"`, and reminder variants for ERCAP requests/proposals being due.
19. **Allocation overuse notice**: match (case-insensitive) `Re:` / `Fw:` / `Fwd:` subjects, with optional `Update Closed Incident:` and optional `[EXTERNAL]`, for `"Utilization exceeding allocation(s) in your project"` or `"Users exceeding their allocation in your project"`.
20. **Allocation award notice**: match (case-insensitive) `Re:` / `Fw:` / `Fwd:` subjects, with optional `Update Closed Incident:` and optional `[EXTERNAL]`, for `"NERSC Allocation Award"` and year-specific `"NERSC AY <year> DOE Mission Science Allocation Award"` / `"Director Reserve Allocation Award"` variants.
21. **Realtime queue access request**: exact match (case-insensitive) `"Realtime Queue Access Request"`.
22. **Node-hour increase request**: prefix match (case-insensitive) `"CPU Node hour increase request for project "` or `"GPU Node hour increase request for project "`.
23. **Travel laptop workflow**: substring match (case-insensitive) containing `"travel laptop"`.
24. **Daily screening alert**: prefix match (case-insensitive) `"Daily RPS Dynamic Screening Alert"`.
25. **SLURM/Iris integration failure**: prefix match (case-insensitive) `"Failure to run slurm_iris.py on "`.
26. **High-load warning**: prefix match (case-insensitive) `"[response required] high load on "`.
27. **Scratch touch policy warning**: exact match (case-insensitive) `"[response required] touching files in your scratch directory"`.
28. **Running-watch policy warning**: exact match (case-insensitive) `"[response required] running watch on NERSC systems"` or equivalent subject variants containing `"running watch on NERSC systems"`.
29. **Training renewal**: regex match (case-insensitive) `^Renewal of .+ Training for Staff$`.
30. **Training expiring**: substring match (case-insensitive) containing `"Training expiring"`.
31. **Account activation**: substring match (case-insensitive) containing `"NERSC Account activation"`.

#### 4.2.3 Message-Level Filters

- Skip messages whose author is `"System"`.
- Skip messages whose text is empty after normalization.

#### 4.2.4 Post-Extraction Filters

After messages are extracted, cleaned, and deduplicated:
- Skip the ticket if zero messages remain.
- Skip the ticket if all remaining messages are authored exclusively by `"autoticketing"`, `"pm-node-info-bot"`, and/or `"system"` (case-insensitive).
- Skip the ticket if exactly one message remains and there are no attachments.

### 4.3 Normalize (`pipeline/normalize.rs`)

Message text is cleaned in the following order. Each step operates on the result of the previous step.

1. **Strip leading metadata**: remove the first non-empty line(s) if they begin with (case-insensitive): `reply from:`, `created by:`, `created by reply`, `updated by reply`, `email received from:`, `received from:`.
2. **Remove greeting**: if the first non-empty line is a greeting (`Hi`, `Hello`, `Hey` with optional name; `Dear <name>`; `Good morning/afternoon/evening` with optional name; `To whom it may concern`), remove it. Punctuation (`,`, `!`, `.`) is optional.
3. **Remove trailing date line**: if the last non-empty line is a standalone date (`YYYY-MM-DD`, `YYYY-MM-DD HH:MM:SS`, `MMM DD, YYYY`, `Month DD, YYYY` with optional timezone) or an email quote header (`On ... at ...`), remove it.
4. **Remove footer lines**: remove any line (quoted `>` or not) matching NERSC footer patterns:
   - `NERSC Account and Allocation Support`
   - `NERSC Account Allocations Support`
   - `NERSC Consulting` (with optional `User Engagement Group (UEG)`)
   - `NERSC User Engagement Group` (with optional `Lead`)
   - `NERSC Data Science Engagement Group`
   - `NERSC Support` (with optional suffix)
   - `Account and Allocation Support` / `Allocations and Account Support` / `Account Support`
   - `NERSC Account Support` (with optional suffix)
   - `NERSC User Support Team`
   - `accounts@nersc.gov`
   - Ohlone and Bay Miwok land acknowledgment
5. **Remove signoff and signature block**: if the last non-empty line is a signoff word (`Best`, `Regards`, `Cordially`, `Thanks`, `Thank you`, `Kind regards`, `Best regards`, `Warm regards`, `Best wishes`, `Many thanks`, `Many thanks and best`, `Thanks in advance`, `Thanks and best`, `All the best`, `All best`, `Sincerely`, `Cheers`, `Bye` — with optional `,` or `.`), remove it. Also handles inline signoff-with-name (e.g. `Best, Joe`). If a name or signature block follows the signoff, remove that too — including name lines, institutional affiliations (department, lab, university, etc.), gender pronouns, email/phone labels, reply headers (`From:`, `Sent:`, `To:`, etc.), and HTML residue.
6. **Remove author name lines**: remove any line (quoted `>` or not) that matches the message author's first name, full name, or `First L.` initial variant.
7. **Trim whitespace**: strip leading and trailing blank lines. Preserve internal structure.

#### 4.3.1 Author Name Normalization

Internal work note authors often include a suffix like `(staff work notes (NERSC private))`. Strip this suffix for display; the `(staff work notes)` label is added by the heading format instead.

### 4.4 Deduplicate (`pipeline/dedup.rs`)

Two consecutive messages are duplicates if:
- Their cleaned text is identical.
- They have the same visibility (both internal or both customer-facing).

Keep only the first occurrence.

### 4.5 PII Filtering (`pii/`)

Controlled by the `pii_filter` config field. Applied after normalization and deduplication.

- `"all"`: redact PII in every message.
- `"asker"`: redact PII only in messages authored by the ticket opener (the first customer-facing message author). Staff messages are left untouched.
- `"none"`: skip PII filtering entirely.

#### 4.5.1 PII Extraction (at load time)

At load time, names, usernames, and name parts are extracted from ticket metadata into a `known_pii` list:
- Raw `created_by` fields from all messages (both customer-facing and internal work notes). Parenthesized usernames like `(ebasheer)` are extracted.
- `incident_fields.caller_id`, `opened_by`, `closed_by`, `resolved_by` — parsed for names (handling `"Last, First (username)"` format) and usernames.
- `incident_fields.sys_created_by` — plain username.
- Individual name parts (first name, last name) are added alongside full names.
- Terms shorter than 3 characters are excluded. Role/generic terms (`system`, `guest`, `operator`, `support`) and all-uppercase organization acronyms (e.g. `NERSC`) are also excluded.

The opener is identified as the author of the first customer-facing (non-internal) message.

#### 4.5.2 Redaction

Redaction is applied in order:
1. **Passwords**: regex matching `password:`, `passwd=`, `passcode:`, `pin:`, `secret:` followed by a value. The label is preserved, the value is replaced with `[PASSWORD]`.
2. **Emails**: standard email pattern, replaced with `[EMAIL]` (or `EMAIL_<HMAC>` in deterministic mode).
3. **Username-in-context patterns**: detect usernames embedded in common NERSC contexts:
   - **Shell logins**: `username@hostname` (e.g. `jsmith@perlmutter`) — replace the username portion.
   - **NERSC home paths**: `/global/homes/u/username`, `/pscratch/sd/u/username`, `/global/cfs/cdirs/project/username` — replace the username portion.
   - **Command user flags**: `-u username` or `--user username` — replace the username portion.
4. **Zoom meeting details**: Zoom URLs (`zoom.us/j/...`), Meeting ID lines, and one-tap dial-in suffixes. Replaced with `[ZOOM]`.
5. **Phone numbers**: conservative pattern requiring country codes, parenthesized area codes, or explicit 3-3-4 digit grouping with separators. Also handles label-gated detection (e.g. `Phone: 5551234567`, `Cell: ...`, `WhatsApp: ...`) for bare digit sequences near phone-related labels. Avoids matching dates or node IDs. Replaced with `[PHONE]`.
6. **Names**: Aho-Corasick case-insensitive dictionary match against the ticket's `known_pii` list. All matches replaced with `[NAME]` (or `USER_<HMAC>` in deterministic mode).

For markdown message headings only, when `deterministic_pii = false`, the opener's redacted author label is rendered as `[ASKER]` instead of `[NAME]`. Non-opener redacted author names remain `[NAME]`.

#### 4.5.3 Deterministic Pseudonymization

When `deterministic_pii = true`, names and emails are replaced with HMAC-SHA256-based pseudonyms instead of generic placeholders:
- Names → `USER_<10-hex-chars>` (e.g. `USER_A3F2B1C9D0`)
- Emails → `EMAIL_<10-hex-chars>` (e.g. `EMAIL_B4E8C2A1F7`)
- Zoom → `[ZOOM]` (always generic)
- Passwords → `[PASSWORD]` (always generic, no identity to link)
- Phones → `[PHONE]` (always generic, no identity to link)

The HMAC uses a fixed salt (`nersc-ticket-processor-v1`) for consistency across runs. The same input always produces the same alias, preserving identity linkage across tickets while still masking the actual PII. The input is lowercased before hashing for case-insensitive consistency.

### 4.6 Timeline (`pipeline/timeline.rs`)

Merge messages and attachments into a single chronological timeline.

Messages in the input JSON are in reverse chronological order (newest first) within each list (`customer_facing_comments` and `internal_work_notes`). Each list is reversed to get chronological order, then the two lists and attachments are merged by timestamp. At equal timestamps, messages come before attachments; original insertion order is preserved as a final tie-breaker.

Consecutive attachment entries are merged into a single `AttachmentGroup`.

### 4.7 Attachments (`pipeline/attachments.rs`)

- **Sanitize filename**: replace `/` and `\` with `_`, remove non-alphanumeric characters (except `.`, `_`, `-`, space), strip leading/trailing spaces and dots, fall back to `"attachment"` if empty.
- **Ensure uniqueness**: append `_2`, `_3`, etc. on collision. Reserved filenames (e.g. `ticket.md` for the markdown format) are defined by the exporter.
- **Write to disk**: write the file from `local_path` (resolved relative to `input_dir`) into the ticket's output directory, either as a symbolic link or as a copied file depending on `symlink_attachments`. When PII filtering is enabled (`pii_filter != "none"`), text attachments are checked for PII before writing: files with known binary extensions (images, archives, documents, etc.) are skipped immediately; all other files are read and, if valid UTF-8, run through the PII redaction pipeline. If PII is found and redacted, the redacted content is written to disk (overriding symlink mode). If no PII is found, the normal copy/symlink path is used. Binary files and non-UTF-8 files are always copied/symlinked without modification. Failures are surfaced as hard errors by the exporter.

### 4.8 Export (`export/`)

The export module dispatches on `output_format` from the config. Each format controls its own output directory layout, file naming, rendering, and attachment-copy order. The pipeline provides a processed `Ticket`; the exporter decides how to write it.

#### 4.8.1 Markdown (`export/markdown.rs`)

The markdown output format (directory layout, `ticket.md` structure, heading rules, attachment listing) is fully specified in [`ticket_format_specification.md`](./ticket_format_specification.md). The export module implements that specification exactly.

**Export steps:**
1. Create the ticket output directory.
2. Resolve attachment filenames (sanitize + uniquify, reserving `ticket.md`).
3. Build the ticket timeline from messages and attachment metadata.
4. Render and write `ticket.md`.
5. Write attachment outputs into the ticket directory. When `symlink_attachments = true`, attachments are written as symbolic links; otherwise they are copied. If an attachment write fails, `ticket.md` is left in place as partial output.

#### 4.8.2 JSON (`export/json.rs`)

The JSON output format preserves the original ServiceNow JSON structure with normalized/deduped discussions and recursive PII sanitization across all fields. This is compatible with the athos filtering system.

**Pipeline differences for JSON:**
- The original parsed `serde_json::Value` is preserved in `ticket.raw_json` at load time.
- Each message is tagged with `source_index` (its position in the original JSON discussion array).
- Message-level PII (step 5) is **skipped** — the recursive JSON PII handles all strings.

**Export steps:**
1. **Write-back**: processed (normalized + deduped) message texts are written back into the `raw_json` discussion arrays. Filtered-out entries (system messages, empty after normalize, dedup'd) are removed from the arrays.
2. **Recursive PII sanitization** (`pii/json.rs`): walks the entire JSON tree and applies:
   - **Structured user fields** (`assigned_to`, `caller_id`, `closed_by`, `created_by`, `opened_by`, `resolved_by`, `reopened_by`, `requested_for`, `sys_created_by`, `sys_updated_by`, `u_owner`, `u_user`, `owner`, `user`) → `USER_<HMAC10>`
   - **Email fields** (`email`, `email_address`, `u_email_watchlist`, `u_email`) → `EMAIL_<HMAC10>`
   - **Watch-list fields** (`u_itil_watch_list`, `u_user_watchlist`, `u_username_watchlist`, `watch_list`) → comma-separated `USER_<HMAC10>` aliases
   - **All other strings** → free-text scan for emails, shell logins, NERSC paths, command user flags, phones, passwords, and Aho-Corasick name dictionary matches
3. **Attachment output**: files are written from `input_dir` to `output_dir` preserving their relative paths (the `local_path` field in the JSON remains valid relative to the output root). Depending on `symlink_attachments`, each output is either a symbolic link or a copied file.
4. **Serialization**: sorted keys, 2-space indentation, matching Python's `json.dump(indent=2, sort_keys=True)`.

**Output path**: preserves the input file's relative path under `output_dir` (e.g. `output_dir/servicenow_incidents/INC022/90/24.json`).

## 5. Parallelism

- The `walkdir` crate discovers JSON files using `d_type` from `readdir()` on Linux, avoiding per-entry `stat()` calls — critical on networked filesystems (Lustre, GPFS, NFS). Discovered paths are collected and processed in parallel via Rayon's `par_iter()`.
- Each ticket is fully independent: load, filter, normalize, dedup, build timeline, write output. No shared mutable state between tickets.
- The pipeline is CPU-bound for normalization and I/O-bound for reads and writes. Rayon's work-stealing scheduler handles both well enough given the volume and the availability of many cores. If I/O contention becomes a bottleneck (unlikely on a parallel filesystem like GPFS/Lustre), a hybrid Rayon + async-I/O approach can be explored later.

## 6. Update Mode

When `mode = "update"`:
1. For JSON output, the output path is computed from the input file path alone (`<output_dir>/<relative_input_path>`) — no JSON parsing needed for the freshness check.
2. For markdown output, the JSON file is read and parsed once. The `incident_number` and `opened_at` fields are extracted to compute the expected output path (`<output_dir>/YYYY/MM/INC########/ticket.md`). The parsed JSON is then reused for the full pipeline (no double-read).
3. Compare the input file's `mtime` against the output file's `mtime`.
4. If the output file does not exist or the input is newer, proceed with full loading and processing. Otherwise skip the file entirely — no further I/O or CPU work.

When `mode = "replace"`:
1. Remove the output directory entirely if it exists.
2. Process all tickets unconditionally.

## 7. Error Handling

The input is trusted and well-formed. Errors indicate data problems that must be surfaced clearly, not silently skipped.

- Malformed JSON or missing required fields: **hard error** with a clear message naming the file path and the field/reason. The ticket is not processed. The error is collected and reported in the final summary.
- Missing attachment file on disk: **hard error** for that ticket. Name the JSON file, attachment name, and expected path.
- I/O errors (read or write): **hard error** for that ticket. Name the path and reason.
- A single ticket's failure does not abort the entire run. The pipeline continues processing other tickets.
- At the end, report a summary: total files found, processed, skipped (filtered), skipped (up-to-date), errored (with the error messages).

## 8. Dependencies (Expected)

| Crate | Purpose |
|-------|---------|
| `serde_json` | JSON parsing |
| `toml` | Config file parsing |
| `rayon` | Parallel iteration |
| `chrono` | Date/time parsing and comparison |
| `regex` | Pattern matching for normalization, filtering, and PII redaction |
| `aho-corasick` | Multi-pattern dictionary matching for PII name redaction |
| `hmac` | HMAC-SHA256 for deterministic PII pseudonymization |
| `sha2` | SHA-256 hash function (used by `hmac`) |
| `hex` | Hex encoding for HMAC output |
| `walkdir` | Recursive directory traversal (uses `d_type`, avoids `stat()`) |

No logging framework — if something goes wrong, print the error to stderr and crash. Summary stats go to stdout at the end of the run.

## 9. Future Work

These are **not** part of the initial implementation but are planned:

- **Direct ServiceNow API integration**: fetch tickets directly from the ServiceNow API instead of reading from pre-exported JSON files.
