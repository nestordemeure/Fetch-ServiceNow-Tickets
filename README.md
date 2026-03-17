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

2. **Filter (ticket-level)**: skip the ticket if its `short_description` matches any of:
   - Exact match: `"Ticket from Iris: New PI Account Request"`.
   - Prefix: `"Storage Quota Increase request:"`.
   - Regex: `^Renewal of .+ Training for Staff$`.
   - Substring: `"NERSC Account activation"`.

3. **Extract and normalize messages**: pull messages from `customer_facing_comments` and `internal_work_notes`. Skip messages authored by `"System"`. Strip the `(staff work notes (NERSC private))` suffix from internal author names. Clean each message's text in order:
   - Strip leading metadata lines (`reply from:`, `created by:`, etc.).
   - Remove a greeting line if first (`Hi`, `Hello`, `Dear <name>`, `Good morning`, etc.).
   - Remove a trailing standalone date or email quote header.
   - Remove NERSC footer lines (`NERSC Consulting`, `accounts@nersc.gov`, etc.).
   - Remove closing signoff (`Best`, `Regards`, `Thanks`, etc.) and optional trailing name.
   - Remove lines matching the author's name.
   - Trim leading/trailing blank lines.
   - Discard messages that are empty after cleaning.

4. **Deduplicate**: remove consecutive messages with identical text and same visibility (internal/customer-facing), keeping the first.

5. **Filter (post-extraction)**: skip the ticket if: zero messages remain, all messages are from bots (`autoticketing`, `pm-node-info-bot`), or exactly one message with no attachments.

6. **Build timeline**: merge messages and attachments into chronological order (sorted by timestamp, messages before attachments at equal timestamps, consecutive attachments grouped). Copy attachment files from `local_path` (resolved relative to `input_dir`), sanitizing filenames for uniqueness.

7. **Export**: render the timeline in the configured output format (currently markdown) and write to `<output_dir>/YYYY/MM/INC########/ticket.md` alongside any attachment files.

## TODO

- Add Athos output data format
- Add PII removal from messages (names, ids, passwords, emails, phone numbers, etc.)
- Import tickets directly from the ServiceNow API
  - Add scron script to refresh tickets regularly
