use crate::error::AppError;

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Plan not found: {0}")]
    PlanNotFound(String),
    #[error("Step execution failed at step {step}: {message}")]
    StepFailed { step: usize, message: String },
    #[error("Execution cancelled")]
    Cancelled,
    #[error("Execution paused")]
    Paused,
    #[error("Max retries exceeded for step {step}")]
    MaxRetries { step: usize },
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
            ExecutionError::Paused => AppError::Execution("Execution paused".to_string()),
            ExecutionError::MaxRetries { step } => {
                AppError::Execution(format!("Max retries exceeded for step {}", step))
            }
            ExecutionError::Internal(msg) => AppError::Execution(msg),
        }
    }
}
