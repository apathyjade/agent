//! 别名管理：运行时的具名版本别名。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::runtime::toolchain::RuntimeType;

/// 管理具名版本别名（例如 "default" → "20.18.3"）。
/// 目前主要维护每个运行时的默认版本。
pub struct AliasManager {
    defaults: Arc<Mutex<HashMap<RuntimeType, String>>>,
}

impl AliasManager {
    /// 创建一个空的别名管理器。
    pub fn new() -> Self {
        Self::with_defaults(HashMap::new())
    }

    /// 创建别名管理器，并预填已持久化的默认版本。
    pub fn with_defaults(defaults: HashMap<RuntimeType, String>) -> Self {
        Self {
            defaults: Arc::new(Mutex::new(defaults)),
        }
    }

    /// 获取某个运行时的默认版本（没有则返回 `None`）。
    pub async fn get_default(&self, rt: &RuntimeType) -> Option<String> {
        let defaults = self.defaults.lock().await;
        defaults.get(rt).cloned()
    }

    /// 设置某个运行时的默认版本。
    pub async fn set_default(&self, rt: &RuntimeType, version: String) {
        let mut defaults = self.defaults.lock().await;
        defaults.insert(rt.clone(), version);
    }

    /// 把具名别名解析为版本字符串。
    /// 目前支持 "default" → 读取默认版本。
    pub async fn resolve_alias(&self, rt: &RuntimeType, alias: &str) -> Option<String> {
        match alias {
            "default" => self.get_default(rt).await,
            _ => {
                // 检查该别名是否以具名别名形式存储
                let defaults = self.defaults.lock().await;
                defaults.get(rt).cloned()
            }
        }
    }

    /// 列出所有已配置的默认版本。
    pub async fn list_defaults(&self) -> HashMap<RuntimeType, String> {
        self.defaults.lock().await.clone()
    }

    /// 清除某个运行时的默认版本。
    pub async fn clear_default(&self, rt: &RuntimeType) {
        let mut defaults = self.defaults.lock().await;
        defaults.remove(rt);
    }
}

impl Default for AliasManager {
    fn default() -> Self {
        Self::new()
    }
}
