//! 官方 `rig-tools` 编码工具接入。
//!
//! 这里不再维护本地文件/执行工具：读写、编辑、搜索、列目录、Shell、删除、移动
//! 全部由 `rig-tools` 提供。它们自带 `annotations` 与按参数计算的 `footprint`，
//! 因此审批与并行冲突判定可以原样复用。
//!
//! 默认行为：相对路径从进程工作目录解析，绝对路径不受限制；写入仍受审批闸门
//! 保护。`shell` 依赖 `sh -c`，Windows 上需要 Git Bash / WSL 提供 `sh`。

use rig_tools::{Delete, Edit, List, Move, Read, Search, Shell, Write};

use crate::agent::tools::convert::{into_dynamic, ToolEntry};

/// 构造全部 rig-tools 工具条目。
pub fn tool_entries() -> Vec<ToolEntry> {
    vec![
        into_dynamic(Read::default()),
        into_dynamic(Write::default()),
        into_dynamic(Edit::default()),
        into_dynamic(Search::default()),
        into_dynamic(List::default()),
        into_dynamic(Shell::default()),
        into_dynamic(Delete::default()),
        into_dynamic(Move::default()),
    ]
}
