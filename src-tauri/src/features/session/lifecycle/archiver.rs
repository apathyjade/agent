use crate::core::error::Result;
use crate::features::session::lifecycle::LifecycleManager;

/// 启动时执行归档检查：把超过配置不活跃天数的会话标记为已归档。
pub async fn run_archive_check(lifecycle: &LifecycleManager) -> Result<()> {
    let cfg = lifecycle.config.lock().await;
    if !cfg.auto_archive_enabled {
        return Ok(());
    }
    let days = cfg.archive_after_days;
    drop(cfg);

    let db = lifecycle.db.lock().await;
    let archivable = db.list_archivable_sessions(days)?;

    for session_id in archivable {
        db.set_session_archived(&session_id, true)?;
    }

    Ok(())
}
