# 常见陷阱

## 项目配置

- **CSP 限制**：`connect-src` 白名单在 `src-tauri/tauri.conf.json` 中——新增 provider 或 base URL 需同步添加
- **`beforeDevCommand`**：Tauri 配置中此项为空——前端开发服务器需手动启动（`cd src-ui && npm run dev`）
- **双 Cargo.lock 风险**：避免在根目录执行 `cargo build`，可能会生成过时的 `Cargo.lock`。始终在 `src-tauri/` 目录下工作
- **`Cargo.lock`**：`src-tauri/Cargo.lock` 已提交（`.gitignore` 中无对应条目），而根目录 `.gitignore` 却有 `Cargo.lock`——操作 git 时注意区分

## 工具系统

- **工具注册**：硬编码在 `ToolRegistry::new()` 中——不支持动态加载（但 SkillManager 可通过 `register_dynamic` 注册脚本工具）
- **缺少测试**：仅有 `tests/tool_calculator.rs`；`Cargo.toml` 中未配置测试运行器

## 运行时管理

- **运行时检测**：`RuntimeManager` 会自动检测系统中已安装的运行时，初次检测可能较慢
- **版本切换**：使用 `switch_runtime_version` 切换版本，内部通过别名（alias）机制实现
- **PATH 冲突**：同一运行时可能存在多个版本在 PATH 中，建议使用 `detect_path_conflicts` 检测

## 消息流

- **`stream_chunk` 事件**：必须在调用 `send_message_stream` 之前完成监听——前端在 `api/tauri.ts` 中处理此逻辑
- **模型配置字段**：`api_key`、`base_url`、`context_window`、`max_tokens` 均为按模型独立存储，非全局
- **上下文窗口**：Agent 循环根据模型的 `context_window` 配置裁剪消息历史，超出部分会被丢弃（保留最近的）

## MCP

- **MCP 服务器**：启动时自动连接配置中的 MCP 服务器，连接失败仅打日志不阻塞启动
- **工具确认模式**：每个 MCP 工具可配置 `auto_allow`（自动允许）/`confirm_once`（每次确认）/`deny`（拒绝）
- **MCP 日志**：通过 `get_mcp_server_logs` 获取服务器 stderr 日志，用于调试连接问题

## 记忆系统

- **种子记忆**：首次启动自动填充 17 条内置记忆，之后不再重复填充
- **检索机制**：基于关键词 OR 匹配（content/tags），按 relevance × access_count × recency 排序
- **上下文注入**：每次发送消息时，从消息内容提取关键词检索最多 5 条相关记忆，注入为 `<remembered_context>` system message

---

# 安全编码规范

1. **敏感信息管理**：严禁在代码中硬编码任何 API Key、密码或 Token。所有凭证必须通过环境变量 (`process.env`) 读取。
2. **防 SQL 注入**：所有数据库操作必须使用参数化查询，禁止任何形式的 SQL 字符串拼接。
3. **日志脱敏**：禁止在日志中打印用户的明文密码和个人隐私信息。

# 第三方依赖安全管理

1. **原生优先原则**：严禁为了实现简单的字符串处理、数组操作或日期格式化而引入大型第三方库。优先使用 ES6+ 原生语法或 Node.js 内置模块。
2. **白名单机制**：
   - HTTP 请求仅限使用 `axios` 或 `fetch`
   - 数据校验仅限使用 `zod`
   - 严禁使用已废弃的 `request`、`moment.js` 等库
3. **强制安全检查**：在建议安装任何新包之前，必须确保该包：
   - 没有已知的高危 CVE 漏洞（需通过 npm audit 验证）
   - 采用宽松的开源协议（MIT / Apache-2.0）
   - 社区活跃度高（近半年有维护记录）
4. **最小化依赖**：拒绝引入体积庞大且功能冗余的"全家桶"式库，优先选择专注单一功能的轻量级模块。
