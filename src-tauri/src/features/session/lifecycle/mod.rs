//! 会话生命周期管理。
//!
//! 汇总自动命名、自动摘要、上下文压缩与归档四项后台能力，共享同一份
//! [`LifecycleConfig`]（持久化在 `settings` 表）与数据库/Provider 句柄。

pub mod config;
pub mod titler;
pub mod summarizer;
pub mod compactor;
pub mod archiver;

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::core::db::repository::Database;
use crate::llm::provider::ProviderRegistry;
use crate::features::session::lifecycle::config::LifecycleConfig;

/// 会话生命周期管理器，向各子模块提供共享依赖。
#[derive(Clone)]
pub struct LifecycleManager {
    pub db: Arc<Mutex<Database>>,
    pub providers: Arc<Mutex<ProviderRegistry>>,
    pub config: Arc<Mutex<LifecycleConfig>>,
}

impl LifecycleManager {
    /// 用共享句柄构造管理器，配置先取默认值，启动后调用
    /// [`LifecycleManager::load_config`] 覆盖。
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

    /// 从 `settings` 表加载配置；缺失或解析失败时保留默认值。
    pub async fn load_config(&self) {
        let db = self.db.lock().await;
        if let Ok(Some(json)) = db.get_setting("lifecycle_config") {
            if let Ok(cfg) = serde_json::from_str::<LifecycleConfig>(&json) {
                *self.config.lock().await = cfg;
            }
        }
    }

    /// 把当前配置序列化写入 `settings` 表。
    pub async fn save_config(&self) -> crate::core::error::Result<()> {
        let cfg = self.config.lock().await;
        let json = serde_json::to_string(&*cfg)?;
        let db = self.db.lock().await;
        db.set_setting("lifecycle_config", &json)?;
        Ok(())
    }
}
