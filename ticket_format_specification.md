# ServiceNow Ticket Storage Spec

This spec defines how to store ServiceNow incidents as files so a path plus a single markdown file is enough to understand the incident.

## Directory Layout

```
/tickets/
  /YYYY/
    /MM/
      /INC########/
        ticket.md
        <attachment files>
```

File Naming:
- `ticket.md` is the canonical summary and timeline.
- Attachment files keep their original extension and must not be named `ticket.md`.

Notes:
- `YYYY/MM` is derived from `opened_at` (fallback to `sys_created_on` if missing).
- `INC########` is the incident number.
- Attachments are stored only for this ticket, in the ticket folder.
- If an attachment filename conflicts with `ticket.md`, rename the attachment (for example, add a numeric suffix).

## Markdown Format (ticket.md)

### Header

Use a top-level heading, then a strict, minimal metadata block.

Example:

```md
# INC0228579 - Perlmutter nid[005848,005908,007104] - Stuck completing -- Downed by ops

- Status: Closed
- Opened: 2025-01-15
- Closed: 2025-01-16
```

Field selection rules:
- Header includes only: Status, Opened, Closed (Closed only if present).
- No other fields are allowed in the header.
- Title: Use `short_description` if present; otherwise just the incident number.

### Timeline / Messages

Each message becomes a level-2 heading with author and visibility.

Format:

```md
## Adam Schultz (internal)

<message text in markdown>
```

Guidance:
- Use `(internal)` for work notes. Customer-facing comments have no label.
- Preserve original text.
- Keep ordering chronological (oldest first) based on each message timestamp.

### Attachments in the Timeline

Attachments have timestamps and are placed into the timeline at their timestamp position. If multiple attachments are consecutive in the timeline, merge them into a single attachment block.

Format:

```md
## Attachments

- `logs.tar.gz`
- `screenshot.png`
```

Rules:
- File names are sanitized to be filesystem-safe and unique per ticket.
- Prefer original filenames with a numeric suffix on collision.
- Keep the attachment list in the markdown so the ticket file is self-contained.
- When a message and attachment share the same timestamp, place attachments after the message.

### Normalization

- Dates: Use only the date portion (`YYYY-MM-DD`) for Opened/Closed.
- Names: Use display names as provided in the raw data.
- Whitespace: Trim trailing spaces, keep intentional line breaks inside messages.

## Example ticket.md file

```md
# INC0000001 - Login failures for project portal

- Status: Resolved
- Opened: 2025-02-10
- Closed: 2025-02-11

## Jane Smith

Users report intermittent login failures after 5pm PT.

## Attachments

- `error_screenshot.png`
- `auth_logs.txt`

## Ops Team (internal)

Identified expired certificate on auth proxy. Rotated cert and restarted.
```
