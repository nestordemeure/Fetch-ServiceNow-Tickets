# Specifications: Rust Rewrite of ServiceNow Ticket Processor

This document specifies the Rust implementation that replaces the Python `format_tickets.py` script. The goal is a parallel, high-throughput ticket processing pipeline that reads ServiceNow incident JSON exports and produces clean, normalized output files.

## 1. Configuration

A TOML configuration file (`config.toml`) at the project root controls all runtime parameters.

```toml
# Path to the directory containing raw ServiceNow incident JSON files.
input_dir = "/home/nestor/Documents/work/NERSC/tickets/demo_ticket_stash/pengfei"

# Path to the directory where processed tickets are written.
output_dir = "./tickets"

# Output format. Currently only "markdown" is supported.
output_format = "markdown"

# Update mode:
#   "replace" - wipe and rebuild the entire output directory.
#   "update"  - only re-process tickets whose source JSON is newer
#               than the corresponding output file (or has no output yet).
mode = "update"
```

All fields are **required**. If any field is missing, the program exits with an error message naming the missing field and listing the valid values (e.g. `output_format` accepts `"markdown"`, `mode` accepts `"update"` or `"replace"`).

## 2. Project Structure

```
src/
  main.rs              - entry point: loads config, orchestrates the pipeline
  config.rs            - TOML config parsing and validation
  types.rs             - shared data structures (Ticket, Message, Attachment, TimelineEntry)
  pipeline/
    mod.rs             - pipeline module, per-ticket processing orchestration
    load.rs            - JSON deserialization of a ticket file into model types
    filter.rs          - ticket-level and message-level filtering rules
    normalize.rs       - message text cleaning (metadata, greetings, footers, signoffs, etc.)
    dedup.rs           - consecutive duplicate message removal
    timeline.rs        - merge messages and attachments into a sorted timeline
    attachments.rs     - attachment extraction, filename sanitization, binary writing
  export/
    mod.rs             - output format dispatch
    markdown.rs        - markdown rendering of a processed ticket
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
- **Incident fields**: `number`, `state`, `opened_at`, `short_description` — plain strings. `closed_at` is optional (ticket may still be open).

### 3.2 Internal Types

Timestamps are parsed once at load time into a single datetime type (e.g. `chrono::NaiveDateTime` or equivalent). No raw strings are kept — the parsed value is used for sorting, comparison, and formatting on output.

```
Ticket {
  incident_number: String,
  short_description: Option<String>,
  status: String,
  opened_date: NaiveDate,
  closed_date: Option<NaiveDate>,
  messages: Vec<Message>,
  attachments: Vec<Attachment>,
}

Message {
  author: String,
  timestamp: NaiveDateTime,
  text: String,
  internal: bool,
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

Each ticket JSON file goes through the following stages in order. The pipeline is applied to all files in parallel. The output format, normalization rules, and filtering rules are also specified in [`ticket_format_specification.md`](./ticket_format_specification.md); this section details the implementation-level behavior.

### 4.1 Load (`pipeline/load.rs`)

- Read the JSON file from disk.
- Deserialize into the internal `Ticket` model. Each field is read from its expected key — no fallback chains.
- Parse timestamps into `NaiveDateTime`. Supported formats: `%Y-%m-%d %H:%M:%S`, `%Y-%m-%dT%H:%M:%S`, and RFC 3339 / ISO 8601.
- Extract the opened date from `opened_at`, closed date from `closed_at`. Both are truncated to `YYYY-MM-DD`. `closed_at` may be absent (ticket still open); all other required fields must be present or the ticket fails with an error identifying the file and field.

### 4.2 Filter (`pipeline/filter.rs`)

#### 4.2.1 Ticket-Level Filters

Skip the entire ticket if any of these match `short_description`:
1. **Iris PI request**: exact match (case-insensitive) `"Ticket from Iris: New PI Account Request"`.
2. **Storage quota increase**: prefix match (case-insensitive) `"Storage Quota Increase request:"`.
3. **Training renewal**: regex match (case-insensitive) `^Renewal of .+ Training for Staff$`.
4. **Training expiring**: substring match (case-insensitive) containing `"Training expiring"`.
5. **Account activation**: substring match (case-insensitive) containing `"NERSC Account activation"`.

#### 4.2.2 Message-Level Filters

- Skip messages whose author is `"System"`.
- Skip messages whose text is empty after normalization.

#### 4.2.3 Post-Extraction Filters

After messages are extracted, cleaned, and deduplicated:
- Skip the ticket if zero messages remain.
- Skip the ticket if all remaining messages are authored exclusively by `"autoticketing"` and/or `"pm-node-info-bot"`.
- Skip the ticket if exactly one message remains and there are no attachments.

### 4.3 Normalize (`pipeline/normalize.rs`)

Message text is cleaned in the following order. Each step operates on the result of the previous step.

1. **Strip leading metadata**: remove the first non-empty line(s) if they begin with (case-insensitive): `reply from:`, `created by:`, `created by reply`, `updated by reply`.
2. **Remove greeting**: if the first non-empty line is a greeting (`Hi`, `Hello`, `Hey` with optional name; `Dear <name>`; `Good morning/afternoon/evening` with optional name), remove it. Punctuation (`,`, `!`, `.`) is optional.
3. **Remove trailing date line**: if the last non-empty line is a standalone date (`YYYY-MM-DD`, `YYYY-MM-DD HH:MM:SS`, `MMM DD, YYYY`, `Month DD, YYYY` with optional timezone) or an email quote header (`On ... at ...`), remove it.
4. **Remove footer lines**: remove any line (quoted `>` or not) matching NERSC footer patterns:
   - `NERSC Account and Allocation Support.`
   - `NERSC Account & Allocations Support.`
   - `NERSC Consulting` (with optional `| User Engagement Group (UEG)`)
   - `NERSC User Engagement Group Lead.`
   - `NERSC Account Support:` (with or without email)
   - `accounts@nersc.gov`
5. **Remove signoff**: if the last non-empty line is a signoff word (`Best`, `Regards`, `Cordially`, `Thanks`, `Thank you`, `Kind regards`, `Best regards`, `Warm regards`, `Best wishes`, `Many thanks`, `Sincerely`, `Cheers` with optional `,` or `.`), remove it. If a name line follows the signoff (possibly separated by blank lines), remove that too.
6. **Remove author name lines**: remove any line (quoted `>` or not) that matches the message author's first name, full name, or `First L.` initial variant.
7. **Trim whitespace**: strip leading and trailing blank lines. Preserve internal structure.

#### 4.3.1 Author Name Normalization

Internal work note authors often include a suffix like `(staff work notes (NERSC private))`. Strip this suffix for display; the `(staff work notes)` label is added by the heading format instead.

### 4.4 Deduplicate (`pipeline/dedup.rs`)

Two consecutive messages are duplicates if:
- Their cleaned text is identical.
- They have the same visibility (both internal or both customer-facing).

Keep only the first occurrence.

### 4.5 Timeline (`pipeline/timeline.rs`)

Merge messages and attachments into a single chronological timeline.

Messages in the input JSON are in reverse chronological order (newest first) within each list (`customer_facing_comments` and `internal_work_notes`). Each list is reversed to get chronological order, then the two lists and attachments are merged by timestamp. At equal timestamps, messages come before attachments; original insertion order is preserved as a final tie-breaker.

Consecutive attachment entries are merged into a single `AttachmentGroup`.

### 4.6 Attachments (`pipeline/attachments.rs`)

- **Sanitize filename**: replace `/` and `\` with `_`, remove non-alphanumeric characters (except `.`, `_`, `-`, space), strip leading/trailing spaces and dots, fall back to `"attachment"` if empty.
- **Ensure uniqueness**: append `_2`, `_3`, etc. on collision. Reserved filenames (e.g. `ticket.md` for the markdown format) are defined by the exporter.
- **Write to disk**: copy the file from `local_path` (resolved relative to `input_dir`) into the ticket's output directory. Hard error if the source file does not exist.

### 4.7 Export (`export/`)

The export module dispatches on `output_format` from the config. Each format controls its own output directory layout, file naming, and rendering. The pipeline provides a processed `Ticket` with a built timeline; the exporter decides how to write it.

#### 4.7.1 Markdown (`export/markdown.rs`)

The markdown output format (directory layout, `ticket.md` structure, heading rules, attachment listing) is fully specified in [`ticket_format_specification.md`](./ticket_format_specification.md). The export module implements that specification exactly.

## 5. Parallelism

- Use **Rayon** for data-parallel iteration over the list of discovered JSON files.
- Each ticket is fully independent: load, filter, normalize, dedup, build timeline, write output. No shared mutable state between tickets.
- The pipeline is CPU-bound for normalization and I/O-bound for reads and writes. Rayon's work-stealing scheduler handles both well enough given the volume and the availability of many cores. If I/O contention becomes a bottleneck (unlikely on a parallel filesystem like GPFS/Lustre), a hybrid Rayon + async-I/O approach can be explored later.

## 6. Update Mode

When `mode = "update"`:
1. Walk the input directory to collect all JSON file paths.
2. **Before loading/processing each file**, do a lightweight pre-parse: read only `metadata.incident_number` and `incident_fields.opened_at` to compute the expected output path (`<output_dir>/YYYY/MM/INC########/ticket.md`).
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
| `regex` | Pattern matching for normalization and filtering |

No logging framework — if something goes wrong, print the error to stderr and crash. Summary stats go to stdout at the end of the run.

## 9. Future Work

These are **not** part of the initial implementation but are planned:

- **Athos output format**: an additional export format for the Athos system (to be specified later).
- **PII removal**: a normalization step that strips personally identifiable information from message text (names, emails, phone numbers, etc.). Will be inserted into the pipeline between normalization and deduplication.
- **Direct ServiceNow API integration**: fetch tickets directly from the ServiceNow API instead of reading from pre-exported JSON files.
