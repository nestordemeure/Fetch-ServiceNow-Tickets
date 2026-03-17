use crate::types::{Attachment, Message, TimelineEntry, TimelineEntryKind};

/// Merge messages and attachments into a single chronological timeline.
/// Messages and attachments are interleaved by timestamp.
/// At equal timestamps, messages come before attachments.
/// Consecutive attachments are merged into a single AttachmentGroup.
pub fn build_timeline(messages: Vec<Message>, attachments: &[Attachment]) -> Vec<TimelineEntry> {
    // Build unsorted entries with an order index for stable sorting
    let mut entries: Vec<TimelineEntry> = Vec::with_capacity(messages.len() + attachments.len());
    let mut order = 0usize;

    for msg in messages {
        entries.push(TimelineEntry {
            timestamp: msg.timestamp,
            kind: TimelineEntryKind::Message {
                author: msg.author,
                text: msg.text,
                internal: msg.internal,
            },
            order,
        });
        order += 1;
    }

    for att in attachments {
        entries.push(TimelineEntry {
            timestamp: att.timestamp,
            kind: TimelineEntryKind::AttachmentGroup(vec![att.resolved_name.clone()]),
            order,
        });
        order += 1;
    }

    // Sort: by timestamp, then messages before attachments, then insertion order
    entries.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| type_priority(&a.kind).cmp(&type_priority(&b.kind)))
            .then_with(|| a.order.cmp(&b.order))
    });

    // Merge consecutive attachment groups
    let mut merged: Vec<TimelineEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        if let TimelineEntryKind::AttachmentGroup(files) = &entry.kind {
            if let Some(last) = merged.last_mut() {
                if let TimelineEntryKind::AttachmentGroup(ref mut prev_files) = last.kind {
                    prev_files.extend(files.clone());
                    continue;
                }
            }
        }
        merged.push(entry);
    }

    merged
}

fn type_priority(kind: &TimelineEntryKind) -> u8 {
    match kind {
        TimelineEntryKind::Message { .. } => 0,
        TimelineEntryKind::AttachmentGroup(_) => 1,
    }
}
