#!/usr/bin/env python3
"""
Build a local tickets folder from ServiceNow incident exports.
"""

from __future__ import annotations

import asyncio
import base64
import binascii
import json
import os
import re
import shutil
import sys
from datetime import datetime
from typing import Any, Dict, Iterable, List, Optional, Tuple


SOURCE_ROOT = "/global/cfs/cdirs/nstaff/dingpf/servicenow_incidents"
OUTPUT_ROOT = os.path.abspath(os.path.join(os.getcwd(), "tickets"))

ATTACHMENT_TIMESTAMP_KEYS = [
    "timestamp",
    "sys_created_on",
    "created_on",
    "created_at",
    "created",
]

ATTACHMENT_NAME_KEYS = [
    "file_name",
    "filename",
    "name",
]

ATTACHMENT_CONTENT_KEYS = [
    "content_base64",
    "payload_base64",
    "file_bytes",
    "content",
    "data",
    "body",
]

MESSAGE_TIMESTAMP_KEYS = [
    "timestamp",
    "sys_created_on",
    "created_on",
    "created_at",
    "created",
]


def date_only(value: str) -> str:
    if " " in value:
        return value.split(" ", 1)[0]
    if "T" in value:
        return value.split("T", 1)[0]
    return value


def parse_timestamp(value: str) -> datetime:
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"):
        try:
            return datetime.strptime(value, fmt)
        except ValueError:
            continue
    try:
        return datetime.fromisoformat(value.replace("Z", ""))
    except ValueError:
        raise ValueError(f"Unparseable timestamp: {value}") from None


def pick_first_key(data: Dict[str, Any], keys: Iterable[str]) -> Optional[str]:
    for key in keys:
        value = data.get(key)
        if value:
            return value
    return None


def require_value(value: Optional[str], context: str) -> str:
    if value is None or value == "":
        raise ValueError(f"Missing required value: {context}")
    return value


def is_trailing_date_line(line: str) -> bool:
    stripped = line.strip()
    if not stripped or len(stripped) > 40:
        return False
    tz_tokens = {"PDT", "PST", "UTC", "GMT"}
    parts = stripped.split()
    if parts and parts[-1] in tz_tokens:
        stripped = " ".join(parts[:-1])
    formats = [
        "%Y-%m-%d",
        "%Y-%m-%d %H:%M:%S",
        "%b %d, %Y",
        "%b %d, %Y, %H:%M",
        "%B %d, %Y",
        "%B %d, %Y, %H:%M",
    ]
    for fmt in formats:
        try:
            datetime.strptime(stripped, fmt)
            return True
        except ValueError:
            continue
    return False


def clean_message_text(text: str, author: Optional[str] = None) -> str:
    lines = text.splitlines()
    filtered = list(lines)

    def normalize_line_token(line: str) -> str:
        stripped = line.strip()
        return re.sub(r"^-+\s*", "", stripped)

    def is_greeting_line(line: str) -> bool:
        stripped = line.strip()
        if not stripped:
            return False
        pattern = (
            r"^(hi|hello|hey)(\s+[^,]+)?[,!:.]?$|"
            r"^dear\s+[^,]+[,!:.]?$|"
            r"^good (morning|afternoon|evening)(\s+[^,]+)?[,!:.]?$"
        )
        return re.match(pattern, stripped, re.IGNORECASE) is not None

    def is_signoff_line(line: str) -> bool:
        stripped = normalize_line_token(line)
        if not stripped:
            return False
        pattern = (
            r"^(best|regards|cordially|thanks|thank you|kind regards|"
            r"best regards|warm regards|best wishes|many thanks|"
            r"sincerely|cheers)[,\.!]?$"
        )
        return re.match(pattern, stripped, re.IGNORECASE) is not None

    def is_name_line(line: str) -> bool:
        stripped = normalize_line_token(line)
        if not stripped:
            return False
        pattern = r"^[A-Za-z][A-Za-z.'-]*(\s+[A-Za-z][A-Za-z.'-]*){0,3}$"
        return re.match(pattern, stripped) is not None

    def is_footer_line(line: str) -> bool:
        stripped = line.strip()
        if not stripped:
            return False
        content = re.sub(r"^>+\s*", "", stripped)
        content = content.strip()
        lower = content.lower()
        footer_patterns = [
            r"^nersc account and allocation support\.?$",
            r"^nersc account & allocations support\.?$",
            r"^nersc consulting(\s*\|{1,2}\s*user engagement group\s*\(ueg\))?\.?$",
            r"^nersc account support:?\s*$",
            r"^nersc account support:\s*accounts@nersc\.gov\.?$",
            r"^accounts@nersc\.gov\.?$",
        ]
        for pattern in footer_patterns:
            if re.match(pattern, lower):
                return True
        return False

    def is_author_name_line(line: str) -> bool:
        if not author:
            return False
        stripped = normalize_line_token(line)
        if not stripped:
            return False
        content = re.sub(r"^>+\s*", "", stripped).strip()
        content = normalize_line_token(content)
        name_base = author.split(" (", 1)[0].strip()
        if not name_base:
            return False
        name_parts = name_base.split()
        first_name = name_parts[0]
        matches = {name_base.lower(), first_name.lower()}
        if len(name_parts) > 1:
            last_initial = name_parts[-1][0]
            matches.add(f"{first_name} {last_initial}".lower())
            matches.add(f"{first_name} {last_initial}.".lower())
        return content.lower() in matches

    # Only remove metadata if it appears as the first non-empty lines.
    def strip_leading_metadata() -> None:
        while True:
            idx = None
            for i, line in enumerate(filtered):
                if line.strip() == "":
                    continue
                idx = i
                break
            if idx is None:
                return
            value = filtered[idx].strip().lower()
            if value.startswith("reply from:"):
                filtered.pop(idx)
                continue
            if value.startswith("created by:"):
                filtered.pop(idx)
                continue
            if value.startswith("created by reply"):
                filtered.pop(idx)
                continue
            if value.startswith("updated by reply"):
                filtered.pop(idx)
                continue
            return

    strip_leading_metadata()

    # Remove greeting line if it is the first non-empty line.
    for i, line in enumerate(filtered):
        if line.strip() == "":
            continue
        if is_greeting_line(line):
            filtered.pop(i)
        break

    # Only remove a trailing date line if it is the last non-empty line.
    last_non_empty = None
    for idx in range(len(filtered) - 1, -1, -1):
        if filtered[idx].strip() != "":
            last_non_empty = idx
            break
    if last_non_empty is not None:
        tail = filtered[last_non_empty].strip()
        if is_trailing_date_line(tail):
            filtered.pop(last_non_empty)
        elif tail.lower().startswith("on ") and " at " in tail.lower():
            filtered.pop(last_non_empty)

    filtered = [line for line in filtered if not is_footer_line(line)]

    # Remove a closing signoff line, with an optional name line immediately after.
    last_non_empty = None
    for idx in range(len(filtered) - 1, -1, -1):
        if filtered[idx].strip() != "":
            last_non_empty = idx
            break
    if last_non_empty is not None:
        if is_signoff_line(filtered[last_non_empty]):
            filtered.pop(last_non_empty)
        else:
            signoff_idx = None
            for idx in range(last_non_empty - 1, -1, -1):
                if filtered[idx].strip() == "":
                    continue
                signoff_idx = idx
                break
            if (
                signoff_idx is not None
                and is_signoff_line(filtered[signoff_idx])
                and is_name_line(filtered[last_non_empty])
            ):
                for idx in range(last_non_empty, signoff_idx - 1, -1):
                    filtered.pop(idx)

    filtered = [line for line in filtered if not is_author_name_line(line)]

    while filtered and filtered[0].strip() == "":
        filtered.pop(0)
    while filtered and filtered[-1].strip() == "":
        filtered.pop()

    return "\n".join(filtered)


def is_iris_pi_request(short_description: Optional[str]) -> bool:
    if not short_description:
        return False
    return short_description.strip().lower() == "ticket from iris: new pi account request"


def is_storage_quota_increase(short_description: Optional[str]) -> bool:
    if not short_description:
        return False
    return short_description.strip().lower().startswith(
        "storage quota increase request:"
    )


def normalize_author(name: str, internal: bool) -> str:
    if internal and " (staff work notes" in name.lower():
        lower = name.lower()
        idx = lower.find(" (staff work notes")
        name = name[:idx].rstrip()
    return name


def sanitize_filename(name: str) -> str:
    name = name.replace("/", "_").replace("\\", "_")
    name = re.sub(r"[^A-Za-z0-9._ -]+", "_", name)
    name = name.strip(" .")
    return name or "attachment"


def unique_filename(name: str, used: set) -> str:
    if name not in used and name != "ticket.md":
        used.add(name)
        return name
    root, ext = os.path.splitext(name)
    counter = 2
    while True:
        candidate = f"{root}_{counter}{ext}"
        if candidate not in used and candidate != "ticket.md":
            used.add(candidate)
            return candidate
        counter += 1


def attachment_bytes(attachment: Dict[str, Any]) -> bytes:
    for key in ATTACHMENT_CONTENT_KEYS:
        value = attachment.get(key)
        if value is None:
            continue
        if isinstance(value, bytes):
            return value
        if isinstance(value, str):
            try:
                return base64.b64decode(value, validate=True)
            except (ValueError, binascii.Error) as exc:
                raise ValueError(
                    f"Attachment content is not valid base64 for key '{key}'"
                ) from exc
    raise ValueError("Attachment content is missing")


def resolve_attachment_timestamp(attachment: Dict[str, Any]) -> str:
    return require_value(
        pick_first_key(attachment, ATTACHMENT_TIMESTAMP_KEYS),
        "attachment timestamp",
    )


def resolve_message_timestamp(message: Dict[str, Any]) -> str:
    return require_value(
        pick_first_key(message, MESSAGE_TIMESTAMP_KEYS),
        "message timestamp",
    )


def write_attachment(
    attachment: Dict[str, Any],
    dest_dir: str,
    used_names: set,
) -> str:
    name = require_value(
        pick_first_key(attachment, ATTACHMENT_NAME_KEYS),
        "attachment filename",
    )
    safe_name = sanitize_filename(name)
    safe_name = unique_filename(safe_name, used_names)
    dest_path = os.path.join(dest_dir, safe_name)

    file_path = attachment.get("file_path")
    if isinstance(file_path, str) and os.path.isfile(file_path):
        shutil.copyfile(file_path, dest_path)
        return safe_name

    payload = attachment_bytes(attachment)
    with open(dest_path, "wb") as handle:
        handle.write(payload)
    return safe_name


def build_ticket_markdown(
    incident_number: str,
    short_description: Optional[str],
    status: Optional[str],
    opened: Optional[str],
    closed: Optional[str],
    timeline: List[Dict[str, Any]],
) -> str:
    title = incident_number
    if short_description:
        title = f"{incident_number} - {short_description}"

    lines = [f"# {title}", ""]
    if status:
        lines.append(f"- Status: {status}")
    if opened:
        lines.append(f"- Opened: {opened}")
    if closed:
        lines.append(f"- Closed: {closed}")
    lines.append("")

    for entry in timeline:
        if entry["type"] == "message":
            heading = entry["author"]
            if entry.get("internal"):
                heading = f"{heading} (staff work notes)"
            lines.extend([f"## {heading}", ""])
            text = entry.get("text") or ""
            lines.append(text.rstrip())
            lines.append("")
        elif entry["type"] == "attachments":
            lines.extend(["## Attachments", ""])
            for name in entry["files"]:
                lines.append(f"- `{name}`")
            lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def build_timeline(
    messages: List[Dict[str, Any]],
    attachments: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    timeline: List[Dict[str, Any]] = []
    index = 0

    for message in messages:
        timestamp = resolve_message_timestamp(message)
        timeline.append(
            {
                "type": "message",
                "timestamp": timestamp,
                "timestamp_dt": parse_timestamp(timestamp),
                "author": require_value(message.get("created_by"), "message author"),
                "internal": message.get("internal", False),
                "text": message.get("text") or "",
                "order": index,
            }
        )
        index += 1

    for attachment in attachments:
        timestamp = resolve_attachment_timestamp(attachment)
        timeline.append(
            {
                "type": "attachment",
                "timestamp": timestamp,
                "timestamp_dt": parse_timestamp(timestamp),
                "file": attachment["resolved_name"],
                "order": index,
            }
        )
        index += 1

    def sort_key(item: Dict[str, Any]) -> Tuple:
        timestamp_dt = item.get("timestamp_dt")
        timestamp_raw = item.get("timestamp") or ""
        kind_priority = 0 if item["type"] == "message" else 1
        return (timestamp_dt, timestamp_raw, kind_priority, item["order"])

    timeline.sort(key=sort_key)

    merged: List[Dict[str, Any]] = []
    buffer_files: List[str] = []
    for item in timeline:
        if item["type"] == "attachment":
            buffer_files.append(item["file"])
            continue
        if buffer_files:
            merged.append({"type": "attachments", "files": buffer_files})
            buffer_files = []
        merged.append(item)

    if buffer_files:
        merged.append({"type": "attachments", "files": buffer_files})

    return merged


def extract_messages(data: Dict[str, Any]) -> List[Dict[str, Any]]:
    discussions = data.get("discussions") or {}
    messages: List[Dict[str, Any]] = []

    for comment in discussions.get("customer_facing_comments", []) or []:
        created_by = comment.get("created_by")
        if isinstance(created_by, str) and created_by.strip().lower() == "system":
            continue
        author = require_value(comment.get("created_by"), "message author")
        comment["text"] = clean_message_text(comment.get("text") or "", author)
        comment["internal"] = False
        comment["created_by"] = normalize_author(author, False)
        messages.append(comment)

    for note in discussions.get("internal_work_notes", []) or []:
        created_by = note.get("created_by")
        if isinstance(created_by, str) and created_by.strip().lower() == "system":
            continue
        author = require_value(note.get("created_by"), "message author")
        note["text"] = clean_message_text(note.get("text") or "", author)
        note["internal"] = True
        note["created_by"] = normalize_author(author, True)
        messages.append(note)

    deduped: List[Dict[str, Any]] = []
    last_text = None
    last_internal = None
    for message in messages:
        text = message.get("text") or ""
        internal = message.get("internal", False)
        if text == last_text and internal == last_internal:
            continue
        deduped.append(message)
        last_text = text
        last_internal = internal

    return deduped


def extract_attachments(data: Dict[str, Any], dest_dir: str) -> List[Dict[str, Any]]:
    attachments = data.get("attachments") or []
    if not isinstance(attachments, list):
        raise ValueError("Attachments must be a list")

    used_names: set = set()
    resolved: List[Dict[str, Any]] = []
    for attachment in attachments:
        if not isinstance(attachment, dict):
            raise ValueError("Attachment entry must be a dict")
        name = write_attachment(attachment, dest_dir, used_names)
        attachment["resolved_name"] = name
        resolved.append(attachment)

    return resolved


def ensure_output_root() -> Optional[asyncio.Task[None]]:
    cleanup_task: Optional[asyncio.Task[None]] = None
    if os.path.isdir(OUTPUT_ROOT):
        timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        old_path = f"{OUTPUT_ROOT}.old-{timestamp}-{os.getpid()}"
        os.rename(OUTPUT_ROOT, old_path)

        async def cleanup() -> None:
            try:
                await asyncio.to_thread(shutil.rmtree, old_path)
            except Exception as exc:
                sys.stderr.write(
                    f"Failed to delete old output folder {old_path}: {exc}\n"
                )

        cleanup_task = asyncio.create_task(cleanup())
    os.makedirs(OUTPUT_ROOT, exist_ok=True)
    return cleanup_task


def write_agents_md() -> None:
    agents_path = os.path.join(OUTPUT_ROOT, "AGENTS.md")
    content = (
        "# NERSC ServiceNow Tickets\n\n"
        f"Tickets are stored at: `{OUTPUT_ROOT}`\n\n"
        "Each ticket has its own folder containing a `ticket.md` file plus any "
        "attachments for that ticket (if present).\n"
        "Attachments live alongside the markdown file in the same folder.\n\n"
        "File structure:\n\n"
        "```\n"
        "/tickets/YYYY/MM/INC########/\n"
        "  ticket.md\n"
        "  <attachment files>\n"
        "```\n\n"
        "While you are not allowed to modify those files, you should search them "
        "for past solutions to problems and other useful information.\n"
    )
    with open(agents_path, "w", encoding="utf-8") as handle:
        handle.write(content)


def iter_json_files(root: str) -> Iterable[str]:
    for dirpath, _, filenames in os.walk(root):
        for filename in filenames:
            if filename.endswith(".json"):
                yield os.path.join(dirpath, filename)


def process_ticket_file(path: str) -> None:
    with open(path, "r", encoding="utf-8") as handle:
        data = json.load(handle)

    metadata = data.get("metadata") or {}
    incident_fields = data.get("incident_fields") or {}

    short_description = incident_fields.get("short_description")
    if is_iris_pi_request(short_description) or is_storage_quota_increase(
        short_description
    ):
        return

    attachments_raw = data.get("attachments") or []
    if not isinstance(attachments_raw, list):
        raise ValueError("Attachments must be a list")

    messages = extract_messages(data)
    if not messages:
        return
    if len(messages) == 1 and not attachments_raw:
        return

    incident_number = require_value(
        metadata.get("incident_number") or incident_fields.get("number"),
        "incident number",
    )
    status = require_value(incident_fields.get("state"), "status")

    opened_raw = require_value(
        incident_fields.get("opened_at") or incident_fields.get("sys_created_on"),
        "opened date",
    )
    closed_raw = incident_fields.get("closed_at") or incident_fields.get(
        "resolved_at"
    )
    opened = date_only(opened_raw)
    closed = date_only(closed_raw) if closed_raw else None

    parts = opened.split("-")
    if len(parts) < 2:
        raise ValueError(f"Opened date is not YYYY-MM-DD: {opened}")
    year, month = parts[0], parts[1]

    ticket_dir = os.path.join(OUTPUT_ROOT, year, month, incident_number)
    os.makedirs(ticket_dir, exist_ok=True)

    attachments = extract_attachments(data, ticket_dir)
    timeline = build_timeline(messages, attachments)

    ticket_md = build_ticket_markdown(
        incident_number=incident_number,
        short_description=short_description,
        status=status,
        opened=opened,
        closed=closed,
        timeline=timeline,
    )

    with open(os.path.join(ticket_dir, "ticket.md"), "w", encoding="utf-8") as handle:
        handle.write(ticket_md)


def render_progress(done: int, total: int) -> None:
    sys.stderr.write(f"\rProcessed {done}/{total} tickets")
    sys.stderr.flush()


async def main_async() -> None:
    if not os.path.isdir(SOURCE_ROOT):
        raise FileNotFoundError(f"Source root not found: {SOURCE_ROOT}")
    cleanup_task = ensure_output_root()

    paths = await asyncio.to_thread(lambda: list(iter_json_files(SOURCE_ROOT)))
    total = len(paths)
    if total == 0:
        raise ValueError(f"No JSON files found under {SOURCE_ROOT}")

    render_progress(0, total)

    max_workers = min(32, (os.cpu_count() or 4) * 2)
    done = 0
    semaphore = asyncio.Semaphore(max_workers)

    async def process_with_limit(path: str) -> None:
        async with semaphore:
            await asyncio.to_thread(process_ticket_file, path)

    tasks = [asyncio.create_task(process_with_limit(path)) for path in paths]
    for task in asyncio.as_completed(tasks):
        await task
        done += 1
        if done == total or done % 50 == 0:
            render_progress(done, total)

    sys.stderr.write("\n")
    write_agents_md()
    if cleanup_task is not None:
        sys.stderr.write("Waiting on old tickets deletion...\n")
        await cleanup_task


if __name__ == "__main__":
    asyncio.run(main_async())
