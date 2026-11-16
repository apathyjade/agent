//! 提示词组装。
//!
//! 把可裁剪的提示词片段按预算拼成最终文本，并返回裁剪轨迹。纯逻辑模块：
//! 不做 IO、不碰数据库、不异步。

pub mod composer;

pub use composer::{compose, ComposedPrompt, Fragment, PromptBudget};
