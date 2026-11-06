use rig::completion::message::{AssistantContent, UserContent};
use rig::completion::Message as ApiMessage;
use crate::core::db::repository::Database;
use crate::core::error::Result;

/// 从消息中提取纯文本，用于估算 token。
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
        ApiMessage::Assistant(turn) => turn
            .content
            .iter()
            .filter_map(|c| match c {
                AssistantContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// 在 token 预算内压缩上下文：系统消息始终保留，会话消息从最新往旧收集，
/// 超限后丢弃更早的部分。
///
/// 返回的消息保持原有顺序。当前实现不读取已有摘要，`_db` / `_session_id`
/// 预留给后续「用摘要替换旧消息」的版本。
pub fn compress_context(
    _db: &Database,
    _session_id: &str,
    messages: Vec<ApiMessage>,
    max_tokens: usize,
) -> Result<Vec<ApiMessage>> {
    if messages.is_empty() {
        return Ok(messages);
    }

    // 把系统消息和会话消息分开
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
        .map(|m| crate::agent::fast_loop::estimate_tokens(&message_text(m)))
        .sum();

    // 仅系统消息就超限时直接截断（理论上少见，但保证不越界）
    if total_tokens > max_tokens {
        return Ok(system_msgs);
    }

    let mut remaining: Vec<ApiMessage> = Vec::new();

    // 从最新到最旧遍历，累积到 token 上限为止
    for msg in conv_msgs.into_iter().rev() {
        let msg_tokens = crate::agent::fast_loop::estimate_tokens(&message_text(&msg));
        if total_tokens + msg_tokens > max_tokens {
            // 超限：丢弃剩余更早的消息
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
