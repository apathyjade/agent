//! 内置工具实现与工具注册表。
//!
//! 每个工具实现 [`rig::tool::Tool`]（唯一编写形态），由 `convert` 转成
//! [`rig::tool::DynamicTool`]（唯一运行时形态），再由 `registry` 统一注册。

pub mod approval;
pub mod base;
pub mod convert;
pub mod policy;
pub mod registry;
pub mod rig_tools;
