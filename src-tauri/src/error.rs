#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Skill error: {0}")]
    Skill(String),

    #[error("Skill validation error: {0}")]
    SkillValidation(String),

    #[error("Execution error: {0}")]
    Execution(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_str())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

impl AppError {
    /// Returns true if the error is retryable (network issues, server errors).
    /// Returns false for auth failures, invalid input, and other non-retryable errors.
    pub fn is_retryable(&self) -> bool {
        match self {
            AppError::Http(e) => {
                // Retry on timeout, connection reset, or server errors (5xx)
                if e.is_timeout() || e.is_connect() || e.is_request() {
                    return true;
                }
                if let Some(status) = e.status() {
                    return status.as_u16() >= 500;
                }
                true // network-level errors are retryable
            }
            AppError::Io(_) => true,
            AppError::Provider(_) => false, // auth/validation errors should not retry
            AppError::Database(_) => false,
            AppError::Serialization(_) => false,
            AppError::Config(_) => false,
            AppError::NotFound(_) => false,
            AppError::InvalidInput(_) => false,
            AppError::Tool(_) => false,
            AppError::Skill(_) => false,
            AppError::SkillValidation(_) => false,
            AppError::Execution(_) => false,
        }
    }
}
