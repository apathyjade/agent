//! 项目环境：绑定项目所需的运行时版本。
//!
//! 位于运行时引擎之上的项目级封装。它扫描项目文件中的运行时需求，并把
//! 绑定关系持久化到应用数据库。底层工具链引擎位于
//! `crate::runtime::toolchain`。

pub mod detector;
pub mod service;

pub use detector::{
    BoundProject, ProjectDetector, ProjectRuntimeRequirement, ProjectScanResult, SyncAction,
    SyncResult,
};
