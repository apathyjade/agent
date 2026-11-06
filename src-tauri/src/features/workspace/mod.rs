//! 工作区能力：git 状态、代码库索引、文件搜索与文件监听。
//!
//! 这些模块只读取磁盘与 git 元数据，不修改用户文件。

pub mod git;
pub mod indexer;
pub mod search;
pub mod watcher;
