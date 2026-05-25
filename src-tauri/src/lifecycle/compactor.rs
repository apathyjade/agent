use crate::api::types::{Message as ApiMessage, MessageRole};
use crate::db::repository::Database;
use crate::error::Result;

/// Replace old message blocks with their summaries where available.
/// Returns optimized message list in the same order (system messages preserved).
pub fn compress_context(
    db: &Database,
    session_id: &str,
    messages: Vec<ApiMessage>,
    max_tokens: usize,
) -> Result<Vec<ApiMessage>> {
    if messages.is_empty() {
        return Ok(messages);
    }

    // Separate system messages from conversation messages
    let system_msgs: Vec<ApiMessage> = messages.iter()
        .filter(|m| m.role == MessageRole::System)
        .cloned()
        .collect();

    let conv_msgs: Vec<ApiMessage> = messages.iter()
        .filter(|m| m.role != MessageRole::System)
        .cloned()
        .collect();

    let mut total_tokens: usize = system_msgs.iter()
        .map(|m| crate::agent::r#loop::AgentLoop::estimate_tokens(&m.content))
        .sum();

    // If system messages alone exceed limit, truncate them (unlikely but safe)
    if total_tokens > max_tokens {
        return Ok(system_msgs);
    }

    // Gather existing summaries for this session
    let summaries = db.get_session_summaries(session_id)?;
    let mut remaining: Vec<ApiMessage> = Vec::new();
    let mut compressed = false;

    // Walk messages from newest to oldest, collecting until we hit the token limit
    for msg in conv_msgs.into_iter().rev() {
        let msg_tokens = crate::agent::r#loop::AgentLoop::estimate_tokens(&msg.content);
        if total_tokens + msg_tokens > max_tokens && !compressed {
            // We need to compress. Find if there's a summary covering this range
            if let Some(ref msg_id) = msg.id {
                if let Some(summary) = summaries.iter()
                    .find(|s| s.message_end_id == *msg_id || s.message_start_id == *msg_id)
                {
                    // Found matching summary - inject it as system context
                    let summary_text = format!(
                        "[Previous conversation compressed: {}]",
                        summary.summary
                    );
                    let summary_tokens = crate::agent::r#loop::AgentLoop::estimate_tokens(&summary_text);
                    total_tokens += summary_tokens;

                    remaining.push(ApiMessage {
                        id: None,
                        role: MessageRole::System,
                        content: summary_text,
                        tool_calls: None,
                        tool_call_id: None,
                    });

                    compressed = true;
                    continue; // Skip the original message, summary replaces a block
                }
            }
            // No summary available, just stop — drop remaining older messages
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
