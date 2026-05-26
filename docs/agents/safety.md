# 安全编码与依赖管理

## 安全编码规范

### 敏感信息管理

- 严禁在代码中硬编码 API Key、密码或 Token
- 所有凭证通过 `process.env` 或 keychain 读取

### 防 SQL 注入

- 所有数据库操作使用参数化查询（`rusqlite::params!`）
- 禁止字符串拼接构建 SQL
- 示例：

```rust
// ✅ 正确
conn.execute("UPDATE sessions SET title = ?1 WHERE id = ?2", params![title, id])?;

// ❌ 禁止
let sql = format!("UPDATE sessions SET title = '{}' WHERE id = '{}'", title, id);
```

### 日志脱敏

- 禁止在日志中打印明文密码、API Key 和个人隐私信息
- Provider 配置输出时遮盖 key：`"sk-...xxxx"`

### 类型安全

- **禁止**：`as any`、`@ts-ignore`、`@ts-expect-error`
- Rust 端优先使用强类型枚举，避免 `String` 泛滥
- TS 严格模式启用 `noUnusedLocals` / `noUnusedParameters` — 未使用参数前加 `_`

---

## 第三方依赖管理

### 原生优先

简单功能优先使用标准库，不引入第三方库：

| 场景 | 原生方案 |
|------|----------|
| 字符串处理 | ES6+ / Rust std |
| 数组操作 | ES6+ / Rust std |
| 日期格式化 | `Intl.DateTimeFormat` / `chrono` |
| HTTP 请求 | `reqwest`（Rust）/ `fetch`（前端） |
| 数据校验 | `zod`（前端）/ `serde`（Rust） |

### 白名单

- HTTP 请求：`reqwest`（Rust）、`fetch`（前端）
- 数据校验：`zod`
- 序列化：`serde` / `serde_json`
- **不使用**已废弃的 `request`、`moment.js`

### 安全检查

引入新包前：

1. 检查 CVE：`cargo audit`（Rust）/ `npm audit`（Node）
2. 检查协议兼容性：MIT / Apache-2.0
3. 检查社区活跃度：GitHub stars、最近更新
4. 优先专注单一功能的轻量级模块，拒绝"全家桶"
