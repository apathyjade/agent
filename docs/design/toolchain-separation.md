# Toolchain 与 Project Env 分层

## 背景

原先 `environment/` 同时承担三件事：

- 本机语言运行时的检测、版本解析、安装、切换、别名；
- 项目目录扫描与 `bound_projects` 数据库持久化；
- Tauri IPC 命令与 CLI 入口。

这导致运行时引擎反向依赖 `AppConfig`、`Database`、`AppState`，CLI 为了执行 `runtime ls` 也会打开应用数据库。本次重构把「工具链引擎」和「桌面应用」切开。

## 分层

```
src-tauri/src/
├── toolchain/          # 独立引擎：无 db / state / commands / config / tauri 依赖
│   ├── services.rs     # RuntimeServices + RuntimeOptions（组合根）
│   ├── detector.rs     # 系统运行时检测
│   ├── installer.rs    # 内置下载安装
│   ├── registry.rs     # 远程版本源
│   ├── sources/        # 各语言版本发现
│   ├── resolver.rs     # 版本约束解析
│   ├── alias.rs        # 应用内默认版本（alias）
│   └── ...
├── project_env/        # 应用侧项目绑定，依赖 db
│   ├── detector.rs     # 扫描项目文件得到运行时要求
│   └── service.rs      # bound_projects 持久化 + 同步
├── cli/
│   └── toolchain.rs    # `agent toolchain ...`（`runtime` 为兼容别名）
└── commands/
    ├── toolchain.rs    # 运行时 IPC 命令
    └── project_env.rs  # 项目绑定 IPC 命令
```

依赖方向：

```
commands / cli ──► toolchain
      │
      └──────────► project_env ──► toolchain
                       │
                       └──────────► db
```

`toolchain` 只允许依赖 `crate::error`（以及第三方 crate）。它不读取 `AppConfig`，而是接收纯值 `RuntimeOptions`：

```rust
pub struct RuntimeOptions {
    pub install_dir: Option<PathBuf>,
    pub download_proxy: Option<String>,
    pub defaults: HashMap<String, String>,
}
```

应用边界通过 `AppConfig::runtime_options()` 做一次转换：

```rust
let toolchain = RuntimeServices::new(config.runtime_options())?;
```

## CLI 的数据库边界

`ToolchainContext` 只持有 `RuntimeServices` 和 `AppConfig`，不持有 `Database`。只有 `toolchain project ...` 子命令在需要时惰性创建数据库：

```rust
ToolchainAction::Project { action } => {
    let db = Arc::new(Mutex::new(Database::new()?));
    project(&self, &db, action).await
}
```

因此 `ls / use / install / uninstall / status / check / default` 不会初始化 SQLite。

## 前端契约保持不变

本次只调整 Rust 侧模块边界，以下 JSON 名称与字段不变：

`RuntimeType`、`RuntimeSource`（`system,built_in,none`）、`RuntimeInfo`、`InstalledVersion`、`InstallProgress`、`RuntimeVersion`、`VersionUpdate`、`DiskUsageItem`、`BoundProject`、`ProjectRuntimeRequirement`、`ProjectScanResult`、`SyncResult`、`VersionManager`。

`RuntimeSource::Unavailable` 仍通过 `#[serde(rename = "none")]` 序列化为 `"none"`。

## CLI 兼容

新命令名为 `agent toolchain`，旧的 `agent runtime` 保留为 `visible_alias`，已有脚本无需修改。
