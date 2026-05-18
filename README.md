# Agent - Cross-platform AI Agent Client

A cross-platform AI agent client built with Rust and Tauri 2.x, supporting multiple LLM backends and extensible tool system.

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
├── src/                    # Rust core
│   ├── api/                # LLM API clients
│   ├── db/                 # SQLite storage
│   ├── tools/              # Agent tool system
│   ├── agent/              # Agent execution loop
│   ├── commands.rs         # Tauri IPC commands
│   └── ...
├── src-ui/                 # React frontend
│   ├── src/
│   │   ├── components/     # UI components
│   │   ├── store/          # Zustand state
│   │   └── ...
│   └── ...
├── Cargo.toml
└── tauri.conf.json
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
