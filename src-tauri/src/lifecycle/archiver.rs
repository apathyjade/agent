use crate::error::Result;
use crate::lifecycle::LifecycleManager;

/// Run archive check on startup. Archives sessions past the configured inactivity threshold.
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
