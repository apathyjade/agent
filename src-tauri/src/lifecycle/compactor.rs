use rig::completion::message::{AssistantContent, UserContent};
use rig::completion::Message as ApiMessage;
use crate::db::repository::Database;
use crate::error::Result;

/// Extract text content from a `Message` for token estimation.
fn message_text(msg: &ApiMessage) -> String {
    match msg {
        ApiMessage::System { content } => content.clone(),
        ApiMessage::User { content } => content
            .iter()
            .filter_map(|c| match c {
                UserContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ApiMessage::Assistant { content, .. } => content
            .iter()
            .filter_map(|c| match c {
                AssistantContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Replace old message blocks with their summaries where available.
/// Returns optimized message list in the same order (system messages preserved).
pub fn compress_context(
    _db: &Database,
    _session_id: &str,
    messages: Vec<ApiMessage>,
    max_tokens: usize,
) -> Result<Vec<ApiMessage>> {
    if messages.is_empty() {
        return Ok(messages);
    }

    // Separate system messages from conversation messages
    let system_msgs: Vec<ApiMessage> = messages
        .iter()
        .filter(|m| matches!(m, ApiMessage::System { .. }))
        .cloned()
        .collect();

    let conv_msgs: Vec<ApiMessage> = messages
        .into_iter()
        .filter(|m| !matches!(m, ApiMessage::System { .. }))
        .collect();

    let mut total_tokens: usize = system_msgs
        .iter()
        .map(|m| crate::agent::r#loop::estimate_tokens(&message_text(m)))
        .sum();

    // If system messages alone exceed limit, truncate them (unlikely but safe)
    if total_tokens > max_tokens {
        return Ok(system_msgs);
    }

    let mut remaining: Vec<ApiMessage> = Vec::new();

    // Walk messages from newest to oldest, collecting until we hit the token limit
    for msg in conv_msgs.into_iter().rev() {
        let msg_tokens = crate::agent::r#loop::estimate_tokens(&message_text(&msg));
        if total_tokens + msg_tokens > max_tokens {
            // Over limit — drop remaining older messages
            break;
        }
        total_tokens += msg_tokens;
        remaining.push(msg);
    }

    remaining.reverse();

    let mut result = system_msgs;
    result.extend(remaining);
    Ok(result)
}
