//! critic 模块：对 worker 产出做质量评审，决定通过、返工或上报。

pub mod agent;

pub use agent::{CriticAgent, Critique, CritiqueDecision, Reviewable};
pub use crate::llm::util::extract_json;
