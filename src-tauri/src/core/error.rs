//! 应用统一错误类型。
//!
//! 所有跨层传播的错误最终都收敛为 [`AppError`]，通过 `?` 运算符向上冒泡。

use serde::Serialize;

/// 应用统一错误类型。
///
/// 每个变体对应一个具体的失败来源，便于日志与排查时定位到正确的子系统。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// SQLite 存储层错误。
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// HTTP 请求或响应处理错误。
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON / YAML 序列化或反序列化错误。
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// 文件系统读写错误。
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// 配置加载或保存错误。
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    /// 模型 provider 调用错误（鉴权失败、参数非法等）。
    #[error("Provider error: {0}")]
    Provider(String),

    /// Rig 框架内部错误。
    #[error("Rig error: {0}")]
    RigError(String),

    /// 工具执行错误。
    #[error("Tool error: {0}")]
    Tool(String),

    /// 人工审批拒绝（或等待超时）。
    #[error("Approval denied: {0}")]
    ApprovalDenied(String),

    /// 目标对象不存在。
    #[error("Not found: {0}")]
    NotFound(String),

    /// 调用方传入的参数非法。
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// 技能（skill）子系统错误。
    #[error("Skill error: {0}")]
    Skill(String),

    /// 技能定义校验失败。
    #[error("Skill validation error: {0}")]
    SkillValidation(String),

    /// 任务执行错误。
    #[error("Execution error: {0}")]
    Execution(String),

    /// 编排器（orchestrator）错误。
    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    /// Worker 执行错误。
    #[error("Worker error: {0}")]
    Worker(String),

    /// 评审（critic）错误。
    #[error("Critic error: {0}")]
    Critic(String),

    /// 工作区（workspace）错误。
    #[error("Workspace error: {0}")]
    Workspace(String),

    /// MCP（Model Context Protocol）服务器连接或工具调用错误。
    ///
    /// 单独成变体而不是复用 [`AppError::Skill`]：日志里出现 `Skill`
    /// 会把排查者引向 `features/skills/`，而实际问题在 MCP 管理器。
    #[error("MCP error: {0}")]
    Mcp(String),
}

impl Serialize for AppError {
    /// 按纯文本序列化，供前端直接展示错误消息。
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_str())
    }
}

/// 本项目统一的结果类型，错误分支固定为 [`AppError`]。
pub type Result<T> = std::result::Result<T, AppError>;

impl AppError {
    /// 判断该错误是否值得重试。
    ///
    /// 网络类故障（超时、连接重置、5xx）与 IO 错误可以重试；
    /// 鉴权失败、参数非法、数据层错误重试无意义。
    pub fn is_retryable(&self) -> bool {
        match self {
            AppError::Http(e) => {
                // 超时、连接失败或请求发送失败都可以重试
                if e.is_timeout() || e.is_connect() || e.is_request() {
                    return true;
                }
                // 服务端错误（5xx）可以重试
                if let Some(status) = e.status() {
                    return status.as_u16() >= 500;
                }
                true // 其余网络层错误也视为可重试
            }
            AppError::Io(_) => true,
            AppError::Provider(_) => false, // 鉴权/参数错误重试无意义
            AppError::RigError(_) => false,
            AppError::Database(_) => false,
            AppError::Serialization(_) => false,
            AppError::Config(_) => false,
            AppError::NotFound(_) => false,
            AppError::InvalidInput(_) => false,
            AppError::Tool(_) => false,
            AppError::ApprovalDenied(_) => false,
            AppError::Skill(_) => false,
            AppError::SkillValidation(_) => false,
            AppError::Execution(_) => false,
            AppError::Orchestrator(_) => false,
            AppError::Worker(_) => false,
            AppError::Critic(_) => false,
            AppError::Workspace(_) => false,
            AppError::Mcp(_) => false,
        }
    }
}
