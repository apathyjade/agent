use crate::core::error::Result;
use crate::features::session::lifecycle::LifecycleManager;

/// 满足以下条件时为会话生成标题：
/// 1. `title_source != 'manual'`
/// 2. 会话至少 2 条消息
/// 3. 尚未生成过自动标题
pub async fn maybe_generate_title(
    lifecycle: &LifecycleManager,
    session_id: &str,
    model_id: &str,
) -> Result<()> {
    let cfg = lifecycle.config.lock().await;
    if !cfg.auto_title_enabled {
        return Ok(());
    }
    // 使用配置的标题模型，否则沿用会话模型
    let actual_model = cfg.title_model.clone().unwrap_or_else(|| model_id.to_string());
    drop(cfg);

    let db = lifecycle.db.lock().await;

    // 仅当 title_source == 'manual'（默认值）时才生成
    let sess = db.get_session(session_id)?
        .ok_or_else(|| crate::core::error::AppError::NotFound("Session gone".to_string()))?;

    // 已自动生成或手动设置过则跳过
    if sess.title_source != "manual" {
        return Ok(());
    }

    let messages = db.get_messages(session_id)?;
    if messages.len() < 2 {
        return Ok(()); // 至少需要一轮 user + assistant
    }

    // 只在标题仍是默认占位时生成
    let title_is_default = sess.title == "新对话" || sess.title.is_empty();
    if !title_is_default {
        return Ok(());
    }

    // 取第一条用户消息与第一条助手回复作为上下文
    let first_user = messages.iter().find(|m| m.role == "user");
    let first_assistant = messages.iter().find(|m| m.role == "assistant");

    let context = match (first_user, first_assistant) {
        (Some(u), Some(a)) => format!("User: {}\nAssistant: {}", u.content, a.content),
        _ => return Ok(()),
    };

    drop(db);

    let prompt = format!(
        "Based on the following conversation, generate a concise title in the user's language (max 8 words). \
         Return ONLY the title, no quotation marks or explanation.\n\nConversation:\n{}",
        context
    );

    let provider = lifecycle.providers.lock().await;
    let p = provider.get(&actual_model)?;
    let title = p.prompt("", &prompt).await?;
    let title = title.trim().trim_matches('"').to_string();
    if !title.is_empty() {
        let db = lifecycle.db.lock().await;
        db.update_session_title_with_source(session_id, &title, "auto_generated")?;
    }

    Ok(())
}
