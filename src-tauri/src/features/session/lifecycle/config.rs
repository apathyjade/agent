use serde::{Deserialize, Serialize};

/// 会话生命周期相关配置，整体以 JSON 存进 `settings` 表。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfig {
    /// 是否在首轮对话后自动生成标题
    pub auto_title_enabled: bool,
    /// 生成标题使用的模型；`None` 表示沿用会话模型
    pub title_model: Option<String>,
    /// 是否自动生成对话摘要
    pub auto_summarize_enabled: bool,
    /// 每次摘要覆盖的消息条数
    pub summarize_chunk_size: usize,
    /// 生成摘要使用的模型；`None` 表示沿用会话模型
    pub summarize_model: Option<String>,
    /// 是否自动归档长期不活跃的会话
    pub auto_archive_enabled: bool,
    /// 超过多少天未活动后归档
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
