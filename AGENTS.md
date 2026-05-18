# AGENTS.md — Agent (Tauri AI Client)

## Build

**The real project is `src-tauri/` — root `Cargo.toml`/`src/` are stale predecessors.**

```bash
# Terminal 1
cd src-ui && npm run dev            # Vite on port 1420 (strict)

# Terminal 2
cd src-tauri && cargo tauri dev      # Tauri, no -- since devCommand is empty

# Production
cd src-tauri && cargo tauri build
```

Frontend build: `cd src-ui && npm run build` runs `tsc && vite build`.

## Dual roots trap

| File | Status |
|------|--------|
| `Cargo.toml` | Stale, lacks `urlencoding`, `tauri-plugin-log` |
| `src/` | Stale, older flat config model |
| `tauri.conf.json` | Has `withGlobalTauri: true`, icon paths with `src-tauri/` prefix |
| `src-tauri/Cargo.toml` | **Actual** manifest |
| `src-tauri/tauri.conf.json` | **Actual** config, has `"$schema"` |
| `src-tauri/Cargo.lock` | Committed (no .gitignore entry) |

## Architecture

- **IPC**: 35+ commands in `src-tauri/src/commands.rs` — `invoke()` from `src-ui/src/api/tauri.ts`, stream via Tauri events (`stream_chunk`)
- **Providers**: 10 hardcoded (`commands.rs:561`). OpenAPI-compatible → `OpenAIProvider`, Anthropic → `AnthropicProvider`
- **Agent loop** (`agent/loop.rs`): retry×3 exp backoff, context window optimization (~4K token budget, summarizes older messages), max 10 tool iterations
- **Tools**: `calculator` (enabled), `file_system` + `web_search` (disabled by default). Register in `ToolRegistry::new()`
- **DB**: `dirs::data_dir()/agent/agent.db` — 4 tables (conversations, messages, settings, system_prompts). Schema migration renames old `provider` column → `model_id`
- **Config**: `dirs::config_dir()/agent/config.json` — `ModelConfig` objects with per-model provider, API key, base URL

## Frontend quirks

- **TS strict**: `noUnusedLocals`/`noUnusedParameters` both on — prefix unused with `_`
- **`ChatArea` references a `models` array** that doesn't exist in the Zustand store — store/API alignment is broken when adding features
- **Single Zustand store** (`src/store/index.ts`): all state + async IPC wrappers
- TailwindCSS with custom `primary` purple palette; utility classes (`btn-primary`, `input-primary`, etc.) in `global.css`

## Gotchas

- **CSP** restricts `connect-src` to OpenAI/Anthropic only — add entries for new providers
- No tests exist anywhere in the repo
- Tool registration is hardcoded in Rust — new tool = new `impl Tool` file + `registry.register()` line
