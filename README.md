# Agent - Cross-platform AI Agent Client

A cross-platform AI agent client built with Rust and Tauri 2.x, supporting multiple LLM backends and extensible tool system.

> 开发文档见 [AGENTS.md](AGENTS.md) 和 [docs/](docs/) 目录。

## Features

- **Cross-platform**: Windows, macOS, Linux, Android, iOS, Web
- **Multi-backend LLM**: OpenAI, Anthropic, and extensible provider system
- **Agent Tools**: Extensible tool system with built-in calculator
- **Conversation Management**: Persistent storage with SQLite
- **Modern UI**: React + TypeScript + TailwindCSS

## Tech Stack

| Layer | Technology |
|-------|------------|
| Framework | Tauri 2.x |
| Backend | Rust (Tokio, reqwest, rusqlite) |
| Frontend | React 18 + TypeScript + Vite |
| Styling | TailwindCSS |
| State | Zustand |
| Database | SQLite |

## Project Structure

```
agent/
├── src-tauri/              # Tauri Rust 后端
│   ├── src/
│   │   ├── agent/          # Agent 执行循环
│   │   ├── api/            # LLM API 客户端
│   │   ├── commands/       # IPC 命令
│   │   ├── db/             # SQLite 存储
│   │   ├── tools/          # 工具系统
│   │   └── ...
│   └── Cargo.toml
├── src-ui/                 # React 前端
│   ├── src/
│   │   ├── components/     # UI 组件
│   │   ├── store/          # Zustand 状态
│   │   └── ...
│   └── ...
├── docs/                   # 项目文档（见 AGENTS.md）
└── scripts/                # AI 工作流脚本
```

## Getting Started

### Prerequisites

- Rust 1.70+
- Node.js 18+
- npm/yarn/pnpm

### Development

```bash
# Install frontend dependencies
cd src-ui
npm install

# Run desktop development
cd ..
cargo tauri dev

# Run Android development
cargo tauri android dev

# Run iOS development
cargo tauri ios dev
```

### VS Code Debugging

共享调试配置在 `.vscode/`（需要安装推荐扩展：rust-analyzer、CodeLLDB）：

| 配置 | 用途 |
|------|------|
| `Tauri: Debug (start Vite)` | 启动 Vite → 用 Cargo 构建并调试 Rust 后端（F5 默认） |
| `Tauri: Debug (Vite already running)` | Vite 已在运行时复用，避免端口 1420 冲突 |
| `Tauri: Attach to Rust process` | 附加到已运行的进程（例如 `cargo tauri dev` 启动的应用） |
| `Tauri: Debug (MSVC, Windows)` | Windows 下改用 MSVC 调试器（需 C/C++ 扩展） |
| `UI: Chrome (Vite)` / `UI: Edge (Vite)` | 调试 React 前端 |

Rust 断点直接打在 `src-tauri/src/**/*.rs`；前端断点打在 `src-ui/src/**/*.ts(x)`。
调试构建会加载 `http://localhost:1420`（见 `src-tauri/tauri.conf.json` 的 `devUrl`）。

### Production Build

```bash
# Desktop
cargo tauri build

# Android
cargo tauri android build

# iOS
cargo tauri ios build
```

## Configuration

API keys are stored in `~/.config/agent/config.json` (Linux/macOS) or `%APPDATA%\agent\config.json` (Windows):

```json
{
  "providers": {
    "openai": {
      "api_key": "sk-...",
      "models": ["gpt-4o", "gpt-4-turbo", "gpt-3.5-turbo"]
    },
    "anthropic": {
      "api_key": "sk-ant-...",
      "models": ["claude-3-opus-20240229", "claude-3-sonnet-20240229"]
    }
  },
  "default_provider": "openai",
  "default_model": "gpt-4o",
  "enabled_tools": ["calculator"]
}
```

## Adding Custom Tools

Implement the `Tool` trait:

```rust
#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "Description" }
    fn parameters(&self) -> Value { /* JSON schema */ }
    async fn execute(&self, input: Value) -> Result<Value> { /* logic */ }
}
```

Register in `ToolRegistry::new()`:

```rust
registry.register("my_tool", Arc::new(MyTool::new()), true);
```

## License

MIT
