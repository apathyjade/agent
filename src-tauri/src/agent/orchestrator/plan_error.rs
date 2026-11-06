//! 执行错误类型，以及到应用级错误 [`AppError`] 的转换。
//!
//! `Display` 文案属于对外契约（会被写入 DB 与展示给前端），保持英文不变。

use crate::core::error::AppError;

/// 计划执行过程中可能出现的错误。
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    /// 指定的计划不存在。
    #[error("Plan not found: {0}")]
    PlanNotFound(String),
    /// 某个步骤执行失败。
    #[error("Step execution failed at step {step}: {message}")]
    StepFailed { step: usize, message: String },
    /// 执行被用户取消。
    #[error("Execution cancelled")]
    Cancelled,
    /// 人工审批拒绝；不可重试（重试只会再次弹窗）。
    #[error("Approval denied: {reason}")]
    ApprovalDenied { reason: String },
    /// 执行被暂停。
    #[error("Execution paused")]
    Paused,
    /// 重试次数耗尽。
    #[error("Max retries exceeded for step {step}")]
    MaxRetries { step: usize },
    /// 运行时内部错误（配置缺失、非法状态等）。
    #[error("Internal execution error: {0}")]
    Internal(String),
}

impl From<ExecutionError> for AppError {
    fn from(e: ExecutionError) -> Self {
        match e {
            ExecutionError::PlanNotFound(id) => {
                AppError::NotFound(format!("Execution plan: {}", id))
            }
            ExecutionError::StepFailed { step, message } => {
                AppError::Execution(format!("Step {} failed: {}", step, message))
            }
            ExecutionError::Cancelled => {
                AppError::Execution("Execution cancelled by user".to_string())
            }
            ExecutionError::ApprovalDenied { reason } => AppError::ApprovalDenied(reason),
            ExecutionError::Paused => AppError::Execution("Execution paused".to_string()),
            ExecutionError::MaxRetries { step } => {
                AppError::Execution(format!("Max retries exceeded for step {}", step))
            }
            ExecutionError::Internal(msg) => AppError::Execution(msg),
        }
    }
}
