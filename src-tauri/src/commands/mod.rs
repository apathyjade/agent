pub mod execution;
pub mod lifecycle;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod orchestrator;
pub mod persona;
pub mod pipeline;
pub mod project;
pub mod project_env;
pub mod prompt;
pub mod provider;
pub mod session;
pub mod settings;
pub mod skill;
pub mod tool;
pub mod toolchain;
pub mod window;

pub use execution::*;
pub use lifecycle::*;
pub use mcp::*;
pub use memory::*;
pub use model::*;
pub use persona::*;
pub use pipeline::*;
pub use project::*;
pub use project_env::*;
pub use prompt::*;
pub use provider::*;
pub use session::*;
pub use settings::*;
pub use skill::*;
pub use tool::*;
pub use toolchain::*;
pub use window::*;

// ── Shared types ──

pub use crate::core::ipc::{StreamChunk, ToolCallEvent};
pub use crate::llm::catalog::{ModelInfo, PROVIDER_OPTIONS, PROVIDER_MODELS};
