# 常见陷阱

## Rig AI 框架

- **Rig 版本兼容**：项目当前使用 `rig = "0.46"`。0.37 → 0.45 是破坏性升级：`ToolDyn` / `CompletionClient` / `StreamingChat` / `ToolError` 均已移除，provider 改为「客户端 → model → `AgentBuilder`」；0.45 → 0.46 的 breaking change 集中在上游 CLI/TUI、plugin/ECS 与 memory API，本项目用到的 `AgentBuilder` / `DynamicTool` / `Message` / extractor 签名未变。迁移细节见 `docs/design/2026-10-29-rig-0.45-migration.md`
- **Provider 非 feature-gated**：Rig 0.46 中所有 provider 内置在 rig-core 中，**没有** `openai`/`anthropic` 等 feature flag。`features = ["openai", "anthropic"]` 会导致构建失败——正确方式：`rig = "0.46"`
- **schemars 版本匹配**：`extract_structured<T>()` 要求 T 实现 `schemars::JsonSchema`。Rig 内部使用 `schemars v1.2.x`，项目中需使用相同版本，否则 trait bound 不满足（错误："trait `schemars::JsonSchema` is not satisfied"）
- **客户端构造不再返回 Result**：`OpenAI::new(key)` / `Anthropic::new(key)` 现在是直接返回客户端；只有 `from_env()` 返回 `Result`。真正的错误边界在 `AgentRunner` 执行时
- **工具不是 `ToolDyn`**：自定义工具实现项目内的 `agent::tools::base::AgentTool`，由 `ToolRegistry::to_rig_tools()` 转成 `rig::tool::DynamicTool`；直接 `impl rig::tool::ToolDyn` 会编译失败
- **流式传输不携带工具调用**：当前流式适配器只透出文本 delta（`StreamEvent::Text`）和 `FinalResponse`。工具调用走 `prompt_with_tools()` 的非流式路径，通过 `ToolLoop` 的输出通道（`StreamEvent`）推送 ToolCall/ToolResult 事件
- **记忆语义搜索依赖 OpenAI API**：`MemoryManager` 的语义搜索需要 OpenAI API key（`OPENAI_API_KEY` 环境变量或 config 中第一个 OpenAI 模型的 key）。无 key 时自动降级到关键词 LIKE 搜索

## 项目配置

- **CSP 限制**：`connect-src` 白名单在 `src-tauri/tauri.conf.json` 中——新增 provider 或修改 base URL 需同步添加
- **`beforeDevCommand` 为空**：Tauri 配置中此项为空——前端开发服务器需手动启动（`cd src-ui && npm run dev`）
- **双 Cargo.lock**：`src-tauri/Cargo.lock` 已提交，而根目录 `.gitignore` 有 `Cargo.lock` 条目。在根目录执行 `cargo build` 会生成过时的 `Cargo.lock`。始终在 `src-tauri/` 目录下工作

## 消息流

- **`stream_chunk` 事件监听时机**：必须在调用 `send_message_stream` 之前完成监听——前端在 `api/tauri.ts` 中处理此逻辑
- **上下文窗口裁剪**：Agent 循环根据模型的 `context_window` 配置裁剪消息历史（保留 system 消息 + 最近消息）

## MCP

- **启动时自动连接**：MCP 服务器启动时自动连接，连接失败仅打日志不阻塞应用启动
- **工具确认模式**：每个 MCP 工具可配置 `auto_allow` / `confirm_once` / `deny`
- **调试手段**：通过 `get_mcp_server_logs` 获取服务器 stderr 日志排查连接问题

## 工具系统

- **静态注册**：内置工具硬编码在 `ToolRegistry::new()` 中；MCP 工具在连接时批量注册、断开时注销。技能不参与工具注册
- **技能只服务外部代理**：技能管理页不做本应用的工具注册，只调用外部 `skills` CLI 安装/移除 SKILL.md 指令包；清单来自 `skills list --json`，前端不自己解析目录
- **测试覆盖**：测试文件在 `src-tauri/tests/` 下（calculator、toolchain\*、project_detector、llm）

## 运行时管理

- **初次检测较慢**：`RuntimeManager` 首次 `detect_all()` 需要遍历系统 PATH 和常见安装目录
- **版本切换机制**：通过版本别名（alias）实现活动版本切换，而非修改系统 PATH
- **PATH 冲突**：同一运行时可能存在多个版本在 PATH 中，用 `detect_path_conflicts` 检测

## 记忆系统

- **种子记忆**：首次启动自动填充 17 条内置记忆。已是"已填充"状态后不再重复填充
- **检索机制**（语义优先）：有 OpenAI API key 时使用 Rig `EmbeddingModel` 进行余弦相似度语义搜索；降级到 SQLite 关键词 LIKE
- **向量索引重建**：应用启动时 `MemoryManager` 从 SQLite 批量重建内存向量索引，首次 `retrieve_relevant()` 调用触发
- **上下文注入**：每次发送消息时自动检索相关记忆，注入为 `<remembered_context>` system message
- **API key 获取**：`MemoryManager` 从 config 中第一个 OpenAI 模型的 api_key 获取；其次尝试 `OPENAI_API_KEY` 环境变量
