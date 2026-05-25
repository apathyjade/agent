use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfig {
    pub auto_title_enabled: bool,
    pub title_model: Option<String>,
    pub auto_summarize_enabled: bool,
    pub summarize_chunk_size: usize,
    pub summarize_model: Option<String>,
    pub auto_archive_enabled: bool,
    pub archive_after_days: u32,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            auto_title_enabled: true,
            title_model: None,
            auto_summarize_enabled: true,
            summarize_chunk_size: 20,
            summarize_model: None,
            auto_archive_enabled: true,
            archive_after_days: 30,
        }
    }
}
