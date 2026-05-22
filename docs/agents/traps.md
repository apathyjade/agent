# 常见陷阱

## 项目配置

- **CSP 限制**：`connect-src` 白名单在 `src-tauri/tauri.conf.json` 中——新增 provider 或修改 base URL 需同步添加
- **`beforeDevCommand` 为空**：Tauri 配置中此项为空——前端开发服务器需手动启动（`cd src-ui && npm run dev`）
- **双 Cargo.lock**：`src-tauri/Cargo.lock` 已提交（`.gitignore` 中无对应条目），而根目录 `.gitignore` 却有 `Cargo.lock`——在根目录执行 `cargo build` 会生成过时的 `Cargo.lock`。始终在 `src-tauri/` 目录下工作

## 消息流

- **`stream_chunk` 事件监听时机**：必须在调用 `send_message_stream` 之前完成监听——前端在 `api/tauri.ts` 中处理此逻辑
- **模型配置按模型独立存储**：`api_key`、`base_url`、`context_window`、`max_tokens` 均为按模型独立存储，非全局共享
- **上下文窗口裁剪**：Agent 循环根据模型的 `context_window` 配置裁剪消息历史（保留 system 消息 + 最近消息）

## MCP

- **启动时自动连接**：MCP 服务器启动时自动连接，连接失败仅打日志不阻塞应用启动
- **工具确认模式**：每个 MCP 工具可配置 `auto_allow` / `confirm_once` / `deny`
- **调试手段**：通过 `get_mcp_server_logs` 获取服务器 stderr 日志排查连接问题

## 工具系统

- **静态注册**：内置工具硬编码在 `ToolRegistry::new()` 中。动态工具通过 SkillManager 的 `register_dynamic` 注册（脚本工具）
- **测试覆盖**：测试文件在 `src-tauri/tests/` 下，已有 4 个测试文件（calculator + runtime* + project_detector）

## 运行时管理

- **初次检测较慢**：`RuntimeManager` 首次 `detect_all()` 需要遍历系统 PATH 和常见安装目录
- **版本切换机制**：通过版本别名（alias）实现活动版本切换，而非修改系统 PATH
- **PATH 冲突**：同一运行时可能存在多个版本在 PATH 中，用 `detect_path_conflicts` 检测

## 记忆系统

- **种子记忆**：首次启动自动填充 17 条内置记忆。已是"已填充"状态后不再重复填充
- **检索机制**：基于关键词 OR 匹配（content/tags），按 relevance × access_count × recency 排序取 top 5
- **上下文注入**：每次发送消息时自动检索相关记忆，注入为 `<remembered_context>` system message

---

# 安全编码规范

1. **敏感信息管理**：严禁在代码中硬编码 API Key、密码或 Token。所有凭证通过 `process.env` 或 keychain 读取
2. **防 SQL 注入**：所有数据库操作使用参数化查询（`rusqlite::params!`），禁止字符串拼接
3. **日志脱敏**：禁止在日志中打印明文密码、API Key 和个人隐私信息
4. **类型安全**：避免 `as any`、`@ts-ignore`、`@ts-expect-error`

# 第三方依赖管理

- **原生优先**：简单的字符串处理、数组操作、日期格式化优先使用 ES6+ / Rust std，不引入第三方库
- **白名单**：HTTP 请求用 `reqwest` / `fetch`；数据校验用 `zod`；不使用已废弃的 `request`、`moment.js`
- **安全检查**：引入新包前检查 CVE（`cargo audit` / `npm audit`）、协议兼容性（MIT / Apache-2.0）、社区活跃度
- **最小化**：拒绝"全家桶"式库，优先专注单一功能的轻量级模块
