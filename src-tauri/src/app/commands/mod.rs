pub mod execution;
pub mod intent;
pub mod lifecycle;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod orchestrate;
pub mod persona;
pub mod project;
pub mod project_env;
pub mod prompt;
pub mod provider;
pub mod session;
pub mod settings;
pub mod skills;
pub mod tool;
pub mod toolchain;
pub mod window;
pub mod workflow;

pub use execution::*;
pub use intent::*;
pub use lifecycle::*;
pub use mcp::*;
pub use memory::*;
pub use model::*;
pub use persona::*;
pub use workflow::*;
pub use project::*;
pub use project_env::*;
pub use prompt::*;
pub use provider::*;
pub use session::*;
pub use settings::*;
pub use skills::*;
pub use tool::*;
pub use toolchain::*;
pub use window::*;

// ── Shared types ──

pub use crate::core::ipc::{StreamChunk, ToolCallEvent};
pub use crate::llm::catalog::{ModelInfo, PROVIDER_OPTIONS, PROVIDER_MODELS};
