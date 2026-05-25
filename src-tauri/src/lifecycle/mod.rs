pub mod config;
pub mod titler;
pub mod summarizer;
pub mod compactor;
pub mod archiver;

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::db::repository::Database;
use crate::api::provider::ProviderRegistry;
use crate::lifecycle::config::LifecycleConfig;

#[derive(Clone)]
pub struct LifecycleManager {
    pub db: Arc<Mutex<Database>>,
    pub providers: Arc<Mutex<ProviderRegistry>>,
    pub config: Arc<Mutex<LifecycleConfig>>,
}

impl LifecycleManager {
    pub fn new(
        db: Arc<Mutex<Database>>,
        providers: Arc<Mutex<ProviderRegistry>>,
    ) -> Self {
        Self {
            db,
            providers,
            config: Arc::new(Mutex::new(LifecycleConfig::default())),
        }
    }

    pub async fn load_config(&self) {
        let db = self.db.lock().await;
        if let Ok(Some(json)) = db.get_setting("lifecycle_config") {
            if let Ok(cfg) = serde_json::from_str::<LifecycleConfig>(&json) {
                *self.config.lock().await = cfg;
            }
        }
    }

    pub async fn save_config(&self) -> crate::error::Result<()> {
        let cfg = self.config.lock().await;
        let json = serde_json::to_string(&*cfg)?;
        let db = self.db.lock().await;
        db.set_setting("lifecycle_config", &json)?;
        Ok(())
    }
}
