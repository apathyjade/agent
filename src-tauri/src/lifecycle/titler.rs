use crate::error::Result;
use crate::lifecycle::LifecycleManager;

/// Generate a title for a session if:
/// 1. title_source != 'manual'
/// 2. session has >= 2 messages
/// 3. no auto title generated yet
pub async fn maybe_generate_title(
    lifecycle: &LifecycleManager,
    session_id: &str,
    model_id: &str,
) -> Result<()> {
    let cfg = lifecycle.config.lock().await;
    if !cfg.auto_title_enabled {
        return Ok(());
    }
    // Use configured title model or fall back to session model
    let actual_model = cfg.title_model.clone().unwrap_or_else(|| model_id.to_string());
    drop(cfg);

    let db = lifecycle.db.lock().await;

    // Check session: only generate if title_source == 'manual' (default)
    let sess = db.get_session(session_id)?
        .ok_or_else(|| crate::error::AppError::NotFound("Session gone".to_string()))?;

    // If already auto-generated or manually set, skip
    if sess.title_source != "manual" {
        return Ok(());
    }

    let messages = db.get_messages(session_id)?;
    if messages.len() < 2 {
        return Ok(()); // Need at least user + assistant
    }

    // Only generate if title is still the default/placeholder
    let title_is_default = sess.title == "新对话" || sess.title.is_empty();
    if !title_is_default {
        return Ok(());
    }

    // Take first user message + first assistant response as context
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
