use crate::types::Message;

/// Remove consecutive duplicate messages.
/// Two messages are duplicates if they have identical text and the same visibility.
pub fn deduplicate(messages: Vec<Message>) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());

    for msg in messages {
        let is_dup = result.last().is_some_and(|prev| {
            prev.text == msg.text && prev.internal == msg.internal
        });
        if !is_dup {
            result.push(msg);
        }
    }

    result
}
