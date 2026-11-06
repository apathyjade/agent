use uuid::Uuid;
use chrono::Utc;

use crate::core::db::models::SessionSummary;
use crate::core::error::Result;
use crate::features::session::lifecycle::LifecycleManager;

/// 为上次摘要之后的新消息生成一段摘要。
///
/// 在 `send_message_stream` 结束后异步调用；消息不足一个 chunk 且会话
/// 尚未变长时直接跳过。
pub async fn maybe_generate_summary(
    lifecycle: &LifecycleManager,
    session_id: &str,
    model_id: &str,
) -> Result<()> {
    let cfg = lifecycle.config.lock().await;
    if !cfg.auto_summarize_enabled {
        return Ok(());
    }
    let chunk_size = cfg.summarize_chunk_size;
    let actual_model = cfg.summarize_model.clone().unwrap_or_else(|| model_id.to_string());
    drop(cfg);

    let db = lifecycle.db.lock().await;

    // 找到上一次摘要结束的位置
    let last_summarized = db.get_latest_summary_end_id(session_id)?;
    let messages = db.get_messages(session_id)?;

    // 统计尚未摘要的 user + assistant 消息
    let start_idx = last_summarized.as_ref().and_then(|end_id| {
        messages.iter().position(|m| m.id == *end_id).map(|i| i + 1)
    }).unwrap_or(0);

    let unsummarized: Vec<&crate::core::db::models::Message> = messages[start_idx..]
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .collect();

    if unsummarized.len() < chunk_size && messages.len() < 50 {
        // 新消息不够一个 chunk，且会话本身还不长
        return Ok(());
    }

    // 最多取 chunk_size 条未摘要消息
    let to_summarize = if unsummarized.len() <= chunk_size {
        // 会话足够长时总是生成摘要
        if messages.len() >= 50 { &unsummarized[..] } else { return Ok(()); }
    } else {
        &unsummarized[..chunk_size]
    };

    if to_summarize.is_empty() {
        return Ok(());
    }

    let first_id = to_summarize.first().unwrap().id.clone();
    let last_id = to_summarize.last().unwrap().id.clone();

    // 拼接待摘要的对话文本
    let conversation_text: String = to_summarize.iter()
        .map(|m| format!("{}: {}", if m.role == "user" { "User" } else { "Assistant" }, m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let original_tokens: i32 = to_summarize.iter()
        .map(|m| crate::agent::fast_loop::estimate_tokens(&m.content) as i32)
        .sum();

    drop(db);

    let prompt = format!(
        "Summarize the key information, decisions, and conclusions from this conversation segment. \
         Be concise but preserve important technical details. Use the same language as the conversation.\n\n{}",
        conversation_text
    );

    let provider = lifecycle.providers.lock().await;
    let p = provider.get(&actual_model)?;
    let summary_text = p.prompt("", &prompt).await?;
    let summary_text = summary_text.trim().to_string();
        let summary_tokens = crate::agent::fast_loop::estimate_tokens(&summary_text) as i32;

    let summary = SessionSummary {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        message_start_id: first_id,
        message_end_id: last_id,
        summary: summary_text,
        key_points: None,
        original_token_count: original_tokens,
        summary_token_count: summary_tokens,
        model_used: Some(actual_model),
        created_at: Utc::now().to_rfc3339(),
    };

    let db = lifecycle.db.lock().await;
    db.insert_summary(&summary)?;

    Ok(())
}
