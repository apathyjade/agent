// ── Alias Manager: Named version aliases for runtimes ──

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::environment::RuntimeType;

/// Manages named version aliases (e.g. "default" → "20.18.3").
/// Primarily manages per-runtime default version.
pub struct AliasManager {
    defaults: Arc<Mutex<HashMap<RuntimeType, String>>>,
}

impl AliasManager {
    pub fn new() -> Self {
        Self {
            defaults: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get the default version for a runtime (or None).
    pub async fn get_default(&self, rt: &RuntimeType) -> Option<String> {
        let defaults = self.defaults.lock().await;
        defaults.get(rt).cloned()
    }

    /// Set the default version for a runtime.
    pub async fn set_default(&self, rt: &RuntimeType, version: String) {
        let mut defaults = self.defaults.lock().await;
        defaults.insert(rt.clone(), version);
    }

    /// Resolve a named alias to a version string.
    /// Currently supports: "default" → get_default()
    pub async fn resolve_alias(&self, rt: &RuntimeType, alias: &str) -> Option<String> {
        match alias {
            "default" => self.get_default(rt).await,
            _ => {
                // Check if the alias is stored as a named alias
                let defaults = self.defaults.lock().await;
                defaults.get(rt).cloned()
            }
        }
    }

    /// List all configured defaults.
    pub async fn list_defaults(&self) -> HashMap<RuntimeType, String> {
        self.defaults.lock().await.clone()
    }

    /// Remove the default for a runtime.
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
