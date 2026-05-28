# Rig-native Refactoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all custom LLM abstractions with native Rig AI framework types and patterns.

**Architecture:** Remove `LLMProvider` trait, custom `Message`/`ChatRequest` types, and custom `Tool` trait. Use `rig::completion::*` types directly, `rig::agent::Agent` for multi-turn execution, and `rig::tool::ToolDyn` for dynamic tool dispatch. Keep `ToolRegistry` as a lightweight facade over `HashMap<String, Arc<dyn ToolDyn>>` with alias resolution. IPC boundary keeps minimal DTOs only where Rig type shapes diverge from frontend expectations.

**Tech Stack:** Rust, Rig v0.37, rig-core v0.37, async-trait, serde, tokio, Tauri 2.x

**Design doc:** `docs/design/2026-05-29-rig-native-refactoring.md`

**Phases (4):**
1. Core types + Provider layer — remove custom types, `LLMProvider` trait
2. Tool system — migrate to `ToolDyn`
3. Agent Loop + LLM call sites — use Rig Agent
4. Orchestrator + Pipeline — direct Rig client usage

---

## File Structure

### Files to modify

| File | Phase | Change summary |
|------|-------|----------------|
| `src-tauri/src/api/types.rs` | 1 | Remove entire file; migrate consumers to `rig::completion::*` |
| `src-tauri/src/api/provider.rs` | 1 | Remove `LLMProvider` trait; change `ProviderRegistry` to store `Box<dyn CompletionClient>` |
| `src-tauri/src/api/rig.rs` | 1 | Remove `RigProvider<C>` wrapper; keep `create_rig_provider()` + `extract_structured<T>()` |
| `src-tauri/src/api/mod.rs` | 1 | Remove `pub mod types` |
| `src-tauri/src/error.rs` | 1 | Add `RigError(String)` variant |
| `src-tauri/src/tools/trait.rs` | 2 | Replace custom `Tool` with `rig::tool::ToolDyn` |
| `src-tauri/src/tools/registry.rs` | 2 | Store `Arc<dyn ToolDyn>`; add `to_rig_tool_set()` |
| `src-tauri/src/tools/calculator.rs` | 2 | Implement `ToolDyn` |
| `src-tauri/src/tools/file_system.rs` | 2 | Implement `ToolDyn` |
| `src-tauri/src/tools/web_search.rs` | 2 | Implement `ToolDyn` |
| `src-tauri/src/tools/code_executor.rs` | 2 | Implement `ToolDyn` |
| `src-tauri/src/tools/run_workflow.rs` | 2 | Implement `ToolDyn` |
| `src-tauri/src/tools/script_tool.rs` | 2 | Implement `ToolDyn` |
| `src-tauri/src/mcp/manager.rs` | 2 | `McpToolWrapper` → `ToolDyn` |
| `src-tauri/src/mcp/bridge.rs` | 2 | `McpToolBridge` → `ToolDyn` |
| `src-tauri/src/skills/mod.rs` | 2 | Update tool registration to `ToolDyn` |
| `src-tauri/src/commands/tool.rs` | 2 | Update if `ToolInfo` type changes |
| `src-tauri/src/agent/loop.rs` | 3 | Rewrite with `rig::agent::Agent` |
| `src-tauri/src/workers/thinker.rs` | 3 | Replace `chat_text()` with Rig Agent call |
| `src-tauri/src/workers/code_explorer.rs` | 3 | Replace `chat_text()` with Rig Agent call |
| `src-tauri/src/workers/code_editor.rs` | 3 | Replace `chat_text()` with Rig Agent call |
| `src-tauri/src/critic/agent.rs` | 3 | Direct `CompletionClient` usage |
| `src-tauri/src/intent/classifier.rs` | 3 | Unify on `extract_structured` |
| `src-tauri/src/lifecycle/summarizer.rs` | 3 | Direct `CompletionClient` usage |
| `src-tauri/src/lifecycle/titler.rs` | 3 | Direct `CompletionClient` usage |
| `src-tauri/src/orchestrator/runtime.rs` | 4 | Direct `CompletionClient` + Agent usage |
| `src-tauri/src/orchestrator/planner.rs` | 4 | Use `extract_structured<ExecutionPlan>` |
| `src-tauri/src/pipeline/engine.rs` | 4 | Direct `CompletionClient` usage |

### Files to create

None — this is a refactoring within existing files.

### Files with no changes needed

- `db/models.rs` — DB types are separate concern, already independent
- `db/repository.rs` — No LLM dependency
- `state.rs` — Holds `ProviderRegistry` which changes internally
- `memory/` — Already mostly Rig-native (uses `EmbeddingsClient`)
- `intent/mod.rs` — Types (`ClassificationResult`) stay
- `intent/router.rs` — No LLM dependency
- `orchestrator/mod.rs` — No change
- `orchestrator/agent.rs` — No LLM call change (uses `Dispatcher`)
- `orchestrator/dispatcher.rs` — No LLM dependency
- `orchestrator/task_graph.rs` — No LLM dependency
- `orchestrator/plan_types.rs` — Types keep working
- `orchestrator/plan_error.rs` — No change
- `orchestrator/event_bridge.rs` — No change
- `orchestrator/pipeline_adapter.rs` — No change
- `workers/mcp_bridge.rs` — No LLM dependency
- `workers/shell.rs` — No LLM dependency
- `workers/web.rs` — No LLM dependency
- `workers/memory.rs` — No LLM dependency
- `pipeline/models.rs` — No change
- `pipeline/scanner.rs` — No change
- `mcp/config.rs` — No change
- `tools/mod.rs` — No change

---

## Phase 1: Core Types + Provider Layer

### Task 1.1: Remove `api/types.rs` and migrate all imports

**Files:**
- Remove: `src-tauri/src/api/types.rs`
- Modify: `src-tauri/src/api/mod.rs`
- Modify: ALL files that import from `crate::api::types`

- [ ] **Step 1: Find all import references to `api/types.rs`**

Run: `Select-String -Path "src-tauri\src\**\*.rs" -Pattern "use crate::api::types"` to list all files referencing `api/types`.

Expected files (non-exhaustive):
- `api/provider.rs`
- `api/rig.rs`
- `agent/loop.rs`
- `commands/session.rs`
- `commands/orchestrator.rs`
- `orchestrator/runtime.rs`
- `orchestrator/planner.rs`
- `critic/agent.rs`
- `pipeline/engine.rs`
- `lifecycle/summarizer.rs`
- `lifecycle/titler.rs`
- `lifecycle/compactor.rs`
- `intent/classifier.rs`

- [ ] **Step 2: Remove `api/types.rs` and update `api/mod.rs`**

In `src-tauri/src/api/mod.rs`, remove the `pub mod types;` line.

- [ ] **Step 3: Update `state.rs` to use Rig types**

The `AppState` in `state.rs` likely uses custom types. Change to `rig::completion::Message` or keep using `db::models::Message` for DB storage (already separate).

- [ ] **Step 4: Update all import lines**

Each file that was `use crate::api::types::{Message, ...}` should become:

```rust
use rig::completion::{Message, ToolCall, ToolDefinition};
// For types that don't have direct Rig equivalents, use rig::completion::*
```

Key type mapping for imports:

| Old import | New import |
|------------|------------|
| `Message` | `rig::completion::Message` |
| `MessageRole` | Not needed — `Message` is an enum with `::User()`, `::Assistant()`, `::ToolResult()` variants |
| `ToolCall` | `rig::completion::ToolCall` |
| `ToolDefinition` | `rig::completion::ToolDefinition` |
| `FunctionDefinition` | Not needed — combined into `ToolDefinition` |
| `ChatRequest` | Not needed — replaced by method parameters |
| `ChatResponse` | Not needed — replaced by `String` return |
| `StreamPayload` | `rig::streaming::StreamedAssistantContent` |
| `Choice` | Not needed — response is direct |
| `Usage` | `rig::completion::Usage` |
| `Session` | Not needed — custom IPC type, keep in commands/ |

- [ ] **Step 5: Check `cargo check`**

Run: `cd src-tauri && cargo check 2>&1`
Expected: Build errors (expected — we're mid-refactoring, errors from callers will be fixed in subsequent steps)

### Task 1.2: Rewrite `api/provider.rs` — Remove `LLMProvider` trait

**File:** `src-tauri/src/api/provider.rs`

- [ ] **Step 1: Replace the trait and registry**

Replace the current file content:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use rig::client::CompletionClient;
use crate::config::AppConfig;
use crate::error::{AppError, Result};
use super::rig::create_rig_provider;

pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn CompletionClient + Send + Sync>>,
    default_model_id: Option<String>,
}

impl ProviderRegistry {
    pub fn new(config: &AppConfig) -> Self {
        let mut providers: HashMap<String, Box<dyn CompletionClient + Send + Sync>> = HashMap::new();

        for model in &config.models {
            if !model.enabled { continue; }
            match create_rig_provider(model) {
                Ok(client) => {
                    providers.insert(model.id.clone(), client);
                }
                Err(e) => {
                    log::warn!("Failed to create Rig provider for '{}' ({}): {}",
                        model.id, model.provider, e);
                }
            }
        }

        let default_id = config.get_default_model().map(|m| m.id.clone());
        Self { providers, default_model_id: default_id }
    }

    pub fn default_model_id(&self) -> &str {
        self.default_model_id.as_deref().unwrap_or("")
    }

    pub fn get(&self, model_id: &str) -> Result<&dyn CompletionClient> {
        self.providers.get(model_id)
            .map(|b| b.as_ref() as &dyn CompletionClient)
            .ok_or_else(|| AppError::Provider(format!("Model '{}' not found", model_id)))
    }

    pub fn resolve(&self, model_id: Option<&str>) -> Result<&dyn CompletionClient> {
        let mid = model_id.unwrap_or_else(|| self.default_model_id());
        if mid.is_empty() {
            return Err(AppError::Provider("No model configured".into()));
        }
        self.get(mid)
    }

    pub fn list_models(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn add_model(&mut self, model: crate::config::ModelConfig) {
        if !model.enabled { return; }
        match create_rig_provider(&model) {
            Ok(client) => { self.providers.insert(model.id.clone(), client); }
            Err(e) => { log::warn!("Failed to create Rig provider: {}", e); }
        }
    }

    pub fn remove_model(&mut self, model_id: &str) {
        self.providers.remove(model_id);
    }

    pub fn is_registered(&self, model_id: &str) -> bool {
        self.providers.contains_key(model_id)
    }

    pub fn get_registered_model_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}
```

- [ ] **Step 2: Remove the `chat_text()` helper function**

Delete the `chat_text()` function at the bottom of `provider.rs`. Callers will use `CompletionClient` directly.

- [ ] **Step 3: Add provider building helper methods**

Add convenience methods for common agent patterns:

```rust
impl ProviderRegistry {
    /// Build a Rig agent with preamble for the given model.
    pub fn build_agent<'a>(
        &self,
        model_id: &str,
        preamble: &'a str,
    ) -> Result<rig::agent::AgentBuilder<'a, &dyn CompletionClient>> {
        let client = self.get(model_id)?;
        Ok(client.agent(model_id).preamble(preamble))
    }
}
```

- [ ] **Step 4: Update `AppState` lock accesses**

In `state.rs`, `AppState` holds `Arc<Mutex<ProviderRegistry>>`. The lock() calls remain the same but the return type changes from `Arc<dyn LLMProvider>` to `&dyn CompletionClient`.

- [ ] **Step 5: Check `cargo check`**

Run: `cd src-tauri && cargo check 2>&1`
Expected: Errors from files still using old API

### Task 1.3: Rewrite `api/rig.rs` — Remove wrapper, keep factory

**File:** `src-tauri/src/api/rig.rs`

- [ ] **Step 1: Remove `RigProvider<C>` struct and its impl blocks**

Remove the entire `RigProvider<C>` struct, all its `impl` blocks, and the `LLMProvider` implementation.

- [ ] **Step 2: Update `create_rig_provider()` return type**

Change from `Box<dyn LLMProvider>` to `Box<dyn CompletionClient + Send + Sync>`:

```rust
pub fn create_rig_provider(model: &ModelConfig) -> Result<Box<dyn CompletionClient + Send + Sync>> {
    match model.provider {
        ModelProvider::OpenAI => {
            let client = rig::providers::openai::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("OpenAI init: {}", e)))?;
            Ok(Box::new(client))
        }
        ModelProvider::Anthropic => {
            let client = rig::providers::anthropic::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("Anthropic init: {}", e)))?;
            Ok(Box::new(client))
        }
        // ... same pattern for each provider
    }
}
```

Each provider branch remains the same — only the return type changes.

- [ ] **Step 3: Remove type aliases**

Remove all `pub type RigOpenAI = ...` aliases — they're no longer needed.

- [ ] **Step 4: Keep `extract_structured<T>()` as-is**

The `extract_structured<T>()` function is already Rig-native. Keep it unchanged.

- [ ] **Step 5: Helper for streaming agent**

Add a helper for creating streaming agents:

```rust
/// Create a Rig agent with preamble for streaming.
pub fn build_stream_agent<'a>(
    providers: &Arc<Mutex<ProviderRegistry>>,
    model_id: &str,
    preamble: &'a str,
) -> Result<rig::agent::AgentBuilder<'a, &'a dyn CompletionClient>> {
    let registry = providers.blocking_lock();  // or use async properly
    let client = registry.get(model_id)?;
    Ok(client.agent(model_id).preamble(preamble))
}
```

- [ ] **Step 6: Remove unused imports**

Remove `use crate::api::types::*` and `use crate::api::provider::LLMProvider`. Remove `use rig::completion::Chat` etc. Use `rig::client::CompletionClient`.

- [ ] **Step 7: Check `cargo check`**

Run: `cd src-tauri && cargo check 2>&1`

### Task 1.4: Update `error.rs` — Add RigError variant

**File:** `src-tauri/src/error.rs`

- [ ] **Step 1: Add RigError variant**

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    // ... existing variants ...
    #[error("Rig error: {0}")]
    RigError(String),
}
```

- [ ] **Step 2: Add From impl for Rig errors (if needed)**

```rust
impl From<rig::error::Error> for AppError {
    fn from(e: rig::error::Error) -> Self {
        AppError::RigError(e.to_string())
    }
}
```

- [ ] **Step 3: Check `cargo check`**

Run: `cd src-tauri && cargo check 2>&1`

### Task 1.5: Fix all callers of old provider API

**Files:** Various — fix compilation errors from Phase 1 changes

- [ ] **Step 1: Run `cargo check` and capture all errors**

```bash
cd src-tauri && cargo check 2>&1 | findstr "error\[" > ../phase1-errors.txt
```

- [ ] **Step 2: Fix each error systematically**

Common patterns to fix:

**Pattern A: `provider.chat(request)` → `agent.chat(&msg, &mut history)`**
```rust
// Before:
let response = provider.chat(request).await?;
let content = response.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();

// After:
let agent = provider.agent(model_id).preamble(&system).build();
let content = agent.chat(&user_message, &mut vec![]).await?;
```

**Pattern B: `Message::system()`, `Message::user()`, etc. → `rig::completion::Message`**
```rust
// Before:
let msg = Message::system("You are helpful");

// After:
let msg = rig::completion::Message::system("You are helpful");
// Note: Rig's Message::system() may not exist — use:
let msg = rig::completion::Message::User(vec![
    rig::completion::UserContent::System("You are helpful".into())
]);
// Or use the convenience helper pattern:
pub fn system_msg(content: &str) -> rig::completion::Message {
    rig::completion::Message::User(vec![
        rig::completion::UserContent::System(content.into())
    ])
}
```

- [ ] **Step 3: Verify `cargo check` passes for Phase 1**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Clean build (Phase 1 files only)

---

## Phase 2: Tool System

### Task 2.1: Update `tools/trait.rs` — Replace with `ToolDyn`

**File:** `src-tauri/src/tools/trait.rs`

- [ ] **Step 1: Replace content with ToolDyn re-export + ToolInfo**

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use rig::tool::ToolDyn;

pub use rig::tool::ToolDyn;

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub enabled: bool,
}
```

- [ ] **Step 2: Remove old `Tool` trait completely**

Delete the `pub trait Tool: Send + Sync { ... }` block.

- [ ] **Step 3: Check `cargo check`**

Expect errors from tool implementations that still implement the old `Tool` trait.

### Task 2.2: Update `tools/registry.rs` — Store `Arc<dyn ToolDyn>`

**File:** `src-tauri/src/tools/registry.rs`

- [ ] **Step 1: Change storage type**

```rust
use std::collections::HashMap;
use std::sync::Arc;
use rig::tool::ToolDyn;
use rig::tools::ToolSet;

use crate::error::{AppError, Result};
use crate::tools::calculator::CalculatorTool;
use crate::tools::code_executor::CodeExecutorTool;
use crate::tools::file_system::FileSystemTool;
use crate::tools::run_workflow::RunWorkflowTool;
use crate::tools::web_search::WebSearchTool;
use crate::tools::r#trait::ToolInfo;

// ... aliases unchanged ...

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolDyn>>,
    enabled: HashMap<String, bool>,
    aliases: HashMap<String, String>,
}
```

- [ ] **Step 2: Update register method**

```rust
pub fn register(&mut self, name: &str, tool: Arc<dyn ToolDyn>, enabled: bool) {
    self.tools.insert(name.to_string(), tool);
    self.enabled.insert(name.to_string(), enabled);
}
```

- [ ] **Step 3: Update `get()` method**

```rust
pub fn get(&self, name: &str) -> Result<Arc<dyn ToolDyn>> {
    if let Some(tool) = self.tools.get(name) {
        return Ok(tool.clone());
    }
    if let Some(alias) = self.aliases.get(name) {
        if let Some(tool) = self.tools.get(alias) {
            return Ok(tool.clone());
        }
    }
    let lower = name.to_lowercase();
    for (key, tool) in &self.tools {
        if key.to_lowercase() == lower {
            return Ok(tool.clone());
        }
    }
    Err(AppError::Tool(format!("Tool '{}' not found", name)))
}
```

- [ ] **Step 4: Add `to_rig_tool_set()` method**

```rust
pub fn to_rig_tool_set(&self, allowed: Option<&[String]>) -> ToolSet {
    let mut builder = rig::tools::ToolSet::builder();
    for (name, tool) in &self.tools {
        let is_enabled = self.enabled.get(name).copied().unwrap_or(false);
        let is_allowed = allowed.map_or(true, |a| a.contains(name));
        if is_enabled && is_allowed {
            builder = builder.tool_dyn(tool.clone());
        }
    }
    builder.build()
}
```

- [ ] **Step 5: Update `execute()` method**

```rust
pub async fn execute(&self, name: &str, input: Value) -> Result<Value> {
    let tool = self.get(name)?;
    tool.call_dyn(input).await
        .map_err(|e| AppError::Tool(format!("Tool '{}' failed: {}", name, e)))
}
```

- [ ] **Step 6: Check `cargo check`**

### Task 2.3: Migrate each tool to `ToolDyn`

**Files:**
- `src-tauri/src/tools/calculator.rs`
- `src-tauri/src/tools/file_system.rs`
- `src-tauri/src/tools/web_search.rs`
- `src-tauri/src/tools/code_executor.rs`
- `src-tauri/src/tools/run_workflow.rs`
- `src-tauri/src/tools/script_tool.rs`

- [ ] **Step 1: Migrate CalculatorTool**

```rust
// Before:
#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str { "calculator" }
    fn description(&self) -> &str { "Perform arithmetic calculations" }
    fn parameters(&self) -> Value { /* JSON Schema */ }
    async fn execute(&self, input: Value) -> Result<Value> { /* ... */ }
}

// After:
use rig::tool::ToolDyn;

#[async_trait]
impl ToolDyn for CalculatorTool {
    fn name(&self) -> &str { "calculator" }
    fn description(&self) -> &str { "Perform arithmetic calculations" }
    fn parameters(&self) -> Value { /* JSON Schema (same as before) */ }
    async fn call_dyn(&self, input: Value) -> std::result::Result<Value, rig::tool::ToolError> {
        let result = self.calculate(input)
            .map_err(|e| rig::tool::ToolError::ToolCallError(e.to_string()))?;
        Ok(result)
    }
}
```

- [ ] **Step 2: Migrate FileSystemTool**

Same pattern — replace `impl Tool` with `impl ToolDyn`, change `execute` to `call_dyn`, wrap errors.

- [ ] **Step 3: Migrate WebSearchTool**

Same pattern.

- [ ] **Step 4: Migrate CodeExecutorTool**

Same pattern.

- [ ] **Step 5: Migrate RunWorkflowTool**

Same pattern.

- [ ] **Step 6: Migrate ScriptTool**

Same pattern. Note: `ScriptTool` is created dynamically, so the `Arc<dyn ToolDyn>` wrapping is natural.

- [ ] **Step 7: Update `register()` calls in `registry.rs`**

Each `registry.register("name", Arc::new(Tool::new()), true)` stays the same — `Arc::new()` still works with `dyn ToolDyn`.

- [ ] **Step 8: Check `cargo check`**

### Task 2.4: Migrate MCP tools to `ToolDyn`

**Files:**
- `src-tauri/src/mcp/manager.rs` (`McpToolWrapper`)
- `src-tauri/src/mcp/bridge.rs` (`McpToolBridge`)

- [ ] **Step 1: Update `McpToolWrapper`**

```rust
#[async_trait]
impl ToolDyn for McpToolWrapper {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn parameters(&self) -> Value { self.parameters.clone() }
    async fn call_dyn(&self, input: Value) -> std::result::Result<Value, rig::tool::ToolError> {
        let result = self.peer.lock().await
            .call_tool(/* ... */)
            .await
            .map_err(|e| rig::tool::ToolError::ToolCallError(e.to_string()))?;
        // Convert result to Value
        Ok(/* result value */)
    }
}
```

- [ ] **Step 2: Update `McpToolBridge`**

Same pattern — `execute` → `call_dyn`, return `ToolError` on failure.

- [ ] **Step 3: Check `cargo check`**

### Task 2.5: Update skill system registration

**File:** `src-tauri/src/skills/mod.rs`

- [ ] **Step 1: Find tool registration calls**

Search for `register_dynamic` or `register` calls. Change from old `Tool` to `ToolDyn`:

```rust
// Before:
self.tools.lock().await.register_dynamic(
    &name, Arc::new(script_tool), enabled);

// After (no change needed if registry accepts Arc<dyn ToolDyn>):
self.tools.lock().await.register_dynamic(
    &name, Arc::new(script_tool), enabled);
```

The interface stays the same if `register_dynamic` now accepts `Arc<dyn ToolDyn>`.

- [ ] **Step 2: Check `cargo check`**

### Task 2.6: Verify Phase 2 builds

- [ ] **Step 1: Run cargo check**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Clean build (assuming Phase 1 is complete)

---

## Phase 3: Agent Loop + LLM Call Sites

### Task 3.1: Rewrite `agent/loop.rs` — Use `rig::agent::Agent`

**File:** `src-tauri/src/agent/loop.rs`

- [ ] **Step 1: Replace imports**

```rust
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use serde::{Serialize, Deserialize};
use rig::client::CompletionClient;
use rig::agent::Agent;

use crate::api::provider::ProviderRegistry;
use crate::error::{AppError, Result};
use crate::tools::registry::ToolRegistry;
```

- [ ] **Step 2: Keep frontend event types**

The `StreamEvent`, `ToolCallInfo`, `ToolResultInfo` types are Tauri IPC-specific. Keep them as-is.

- [ ] **Step 3: Rewrite `run()` method**

```rust
pub async fn run(
    &self,
    model_id: &str,
    system_prompt: &str,
    user_message: &str,
    tools_enabled: bool,
    allowed_tools: Option<Vec<String>>,
) -> Result<String> {
    let provider = {
        let registry = self.providers.lock().await;
        // Need to clone the Box<dyn CompletionClient> or use a reference
        // with proper lifetime handling
        // Option A: Use a client reference approach
        let client: &dyn CompletionClient = registry.get(model_id)?;
        // Build agent in scope
    };

    // Build agent with tools
    let mut agent_builder = provider.agent(model_id)
        .preamble(system_prompt);

    if tools_enabled {
        let tool_set = {
            let reg = self.tools.lock().await;
            reg.to_rig_tool_set(allowed_tools.as_deref())
        };
        agent_builder = agent_builder.tool_set(tool_set);
    }

    let agent = agent_builder.build();

    let response = agent.prompt(user_message).await
        .map_err(|e| AppError::RigError(e.to_string()))?;

    Ok(response)
}
```

**Note:** `rig::agent::Agent::prompt()` handles multi-turn tool execution internally. No manual loop needed.

- [ ] **Step 4: Rewrite `run_stream()` method**

For streaming with tools, use `agent.stream_prompt()`:

```rust
pub async fn run_stream(
    &self,
    model_id: &str,
    system_prompt: &str,
    user_message: &str,
    tools_enabled: bool,
    allowed_tools: Option<Vec<String>>,
    tx: mpsc::Sender<StreamEvent>,
) -> Result<()> {
    let provider = self.providers.lock().await;
    let client: &dyn CompletionClient = provider.get(model_id)?;

    let mut agent_builder = client.agent(model_id)
        .preamble(system_prompt);

    if tools_enabled {
        let tool_set = {
            let reg = self.tools.lock().await;
            reg.to_rig_tool_set(allowed_tools.as_deref())
        };
        agent_builder = agent_builder.tool_set(tool_set);
    }

    let agent = agent_builder.build();

    let mut stream = agent.stream_prompt(user_message).await
        .map_err(|e| AppError::RigError(e.to_string()))?;

    while let Some(item) = stream.next().await {
        match item {
            rig::agent::MultiTurnStreamItem::StreamAssistantItem(content) => {
                match content {
                    rig::streaming::StreamedAssistantContent::Text(text) => {
                        let _ = tx.send(StreamEvent::Content(text.text)).await;
                    }
                    rig::streaming::StreamedAssistantContent::ToolCall(tc) => {
                        let _ = tx.send(StreamEvent::ToolCall(ToolCallInfo {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                        })).await;
                    }
                    _ => {}
                }
            }
            rig::agent::MultiTurnStreamItem::ToolCallResult(tc_result) => {
                let _ = tx.send(StreamEvent::ToolResult(ToolResultInfo {
                    call_id: tc_result.id,
                    name: tc_result.name,
                    result: tc_result.output,
                })).await;
            }
            rig::agent::MultiTurnStreamItem::FinalResponse(response) => {
                let _ = tx.send(StreamEvent::Content(response)).await;
                break;
            }
            _ => {}
        }
    }

    let _ = tx.send(StreamEvent::Done).await;
    Ok(())
}
```

- [ ] **Step 5: Remove unused methods**

Remove `optimize_context()`, `estimate_tokens()`, `retry_with_backoff()`, `run_stream_inner()`, `execute_tool()`.

- [ ] **Step 6: Keep test helpers for existing tests**

Update test code to use new API:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Remove tests that used optimize_context/estimate_tokens
    // Keep or adapt tests for general behavior
}
```

- [ ] **Step 7: Check `cargo check`**

### Task 3.2: Update `workers/thinker.rs` — Direct Rig client call

**File:** `src-tauri/src/workers/thinker.rs`

- [ ] **Step 1: Replace `chat_text()` call**

```rust
// Before:
let content = chat_text(
    &self.providers,
    task.model_id.as_deref(),
    &system_prompt,
    &task.instruction,
    task.max_tokens.map(|t| t as usize),
    task.temperature,
).await?;

// After:
let content = {
    let registry = self.providers.lock().await;
    let client = registry.resolve(task.model_id.as_deref())?;
    let agent = client.agent(client.default_model_id())  // Use the model ID from registry
        .preamble(&system_prompt)
        .temperature(task.temperature.unwrap_or(0.7) as f64)
        .max_tokens(task.max_tokens.unwrap_or(4096) as u64)
        .build();
    agent.chat(&task.instruction, &mut vec![]).await
        .map_err(|e| crate::error::AppError::RigError(e.to_string()))?
};
```

- [ ] **Step 2: Check `cargo check`**

### Task 3.3: Update `workers/code_explorer.rs`

**File:** `src-tauri/src/workers/code_explorer.rs`

- [ ] **Step 1: Replace `chat_text()` call**

Same pattern as Task 3.2 — replace `chat_text()` with `agent.chat()`.

- [ ] **Step 2: Check `cargo check`**

### Task 3.4: Update `workers/code_editor.rs`

**File:** `src-tauri/src/workers/code_editor.rs`

- [ ] **Step 1: Replace `chat_text()` call**

Same pattern.

- [ ] **Step 2: Check `cargo check`**

### Task 3.5: Update `critic/agent.rs`

**File:** `src-tauri/src/critic/agent.rs`

- [ ] **Step 1: Replace `provider.chat(request)` with Rig agent call**

```rust
// Before:
let request = ChatRequest { messages, model, tools: None, stream: Some(false), ... };
let response = provider.chat(request).await?;

// After:
let agent = provider.agent(model_id)
    .preamble(CRITIC_SYSTEM_PROMPT)
    .max_tokens(1024)
    .temperature(0.2)
    .build();
let raw = agent.chat(&user_prompt, &mut vec![]).await
    .map_err(|e| crate::error::AppError::RigError(e.to_string()))?;
```

- [ ] **Step 2: Remove unused imports**

Remove `ChatRequest`, `Message`, `MessageRole` imports.

- [ ] **Step 3: Check `cargo check`**

### Task 3.6: Update `intent/classifier.rs`

**File:** `src-tauri/src/intent/classifier.rs`

- [ ] **Step 1: Remove `llm_classify()` fallback**

Remove the `llm_classify()` method entirely. The `rig_classify()` method already uses `extract_structured` which is Rig-native.

```rust
pub async fn classify(&self, message: &str) -> ClassificationResult {
    // Try Rig extractor first
    if !self.openai_api_key.is_empty() {
        match self.rig_classify(message).await {
            Ok(result) => return result,
            Err(e) => log::warn!("Rig classify failed: {}", e),
        }
    }
    // Keep only keyword fallback for offline
    fallback_classify(message)
}
```

- [ ] **Step 2: Remove unused imports and `ProviderRegistry` field**

If `llm_classify` was the only use of `ProviderRegistry` in the classifier, remove that field.

- [ ] **Step 3: Check `cargo check`**

### Task 3.7: Update `lifecycle/summarizer.rs`

**File:** `src-tauri/src/lifecycle/summarizer.rs`

- [ ] **Step 1: Replace `provider.chat(request)` with Rig agent call**

```rust
// Before:
let request = ChatRequest { messages: vec![Message { ... }], model, ... };
let provider = lifecycle.providers.lock().await;
let p = provider.get(&actual_model)?;
let response = p.chat(request).await?;

// After:
let provider = lifecycle.providers.lock().await;
let client = provider.get(&actual_model)?;
let agent = client.agent(&actual_model)
    .max_tokens(500)
    .temperature(0.3)
    .build();
let response = agent.chat(&prompt, &mut vec![]).await
    .map_err(|e| crate::error::AppError::RigError(e.to_string()))?;
```

- [ ] **Step 2: Remove `ChatRequest`, `Message`, `MessageRole` imports**

- [ ] **Step 3: Remove `AgentLoop::estimate_tokens()` reference**

Replace with a simpler token estimate or use Rig's token counting if available.

- [ ] **Step 4: Check `cargo check`**

### Task 3.8: Update `lifecycle/titler.rs`

**File:** `src-tauri/src/lifecycle/titler.rs`

- [ ] **Step 1: Same pattern as summarizer**

Replace `provider.chat()` with `agent.chat()`.

- [ ] **Step 2: Check `cargo check`**

### Task 3.9: Update `commands/session.rs` — IPC command entry points

**File:** `src-tauri/src/commands/session.rs`

- [ ] **Step 1: Update `send_message` and `send_message_stream`**

These commands create `AgentLoop` and call `run()`/`run_stream()`. Update construction to match new API:

```rust
// Before:
let agent = AgentLoop::new(state.providers.clone(), state.tools.clone());
let response = agent.run(&model_id, messages, tools_enabled, allowed_tools).await?;

// After:
let agent = ToolLoop::new(state.providers.clone(), state.tools.clone());
let response = agent.run(&model_id, system_prompt, &user_message, tools_enabled, allowed_tools).await?;
```

- [ ] **Step 2: Check `cargo check`**

### Task 3.10: Fix remaining callers

**Files:** `src-tauri/src/lifecycle/compactor.rs`, any other file using old types

- [ ] **Step 1: Run `cargo check` and fix each error**

```bash
cd src-tauri && cargo check 2>&1 | findstr /R "error\[E"
```

- [ ] **Step 2: Fix each error**

Common fixes:
- `Message::system("...")` → `rig::completion::Message::User(vec![UserContent::System("...".into())])`
- `Message::user("...")` → `rig::completion::Message::User(vec![UserContent::Text("...".into())])`
- `Message::assistant("...", calls)` → `rig::completion::Message::Assistant(vec![AssistantContent::Text("...".into())])`
- `MessageRole::System` → not needed, role encoded in enum variant

- [ ] **Step 3: Final `cargo check` for Phase 3**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Clean build

---

## Phase 4: Orchestrator + Pipeline

### Task 4.1: Update `orchestrator/runtime.rs`

**File:** `src-tauri/src/orchestrator/runtime.rs`

- [ ] **Step 1: Update `execute_llm()` method**

```rust
// Before:
let provider = providers.lock().await.get(mid)?;
let request = ChatRequest { messages, model, tools, ... };
let response = provider.chat(request).await?;

// After:
let client: &dyn CompletionClient = providers.lock().await.get(mid)?;
let agent = client.agent(&mid)
    .preamble(system_prompt.as_deref().unwrap_or(""))
    .temperature(temperature.unwrap_or(0.7) as f64)
    .max_tokens(max_tokens.unwrap_or(4096) as u64)
    .build();
let content = agent.chat(&user_prompt, &mut vec![]).await
    .map_err(|e| ExecutionError::StepFailed { step: 0, message: format!("LLM call failed: {}", e) })?;
```

- [ ] **Step 2: Update `run_agent()` method**

Replace the inner `AgentLoop::new()` with direct Rig agent:

```rust
// Before:
let mut agent = AgentLoop::new(self.providers.clone(), self.tools.clone());
agent.run(&mid, messages, tools_enabled, allowed_tools).await?;

// After:
let client: &dyn CompletionClient = self.providers.lock().await.get(&mid)?;
let mut agent_builder = client.agent(&mid)
    .preamble(system_prompt);
if tools_enabled {
    let tool_set = self.tools.lock().await.to_rig_tool_set(allowed_tools.as_deref());
    agent_builder = agent_builder.tool_set(tool_set);
}
let agent = agent_builder.build();
let content = agent.prompt(&instruction).await
    .map_err(|e| ExecutionError::StepFailed { ... })?;
```

- [ ] **Step 3: Remove unused imports**

Remove `AgentLoop`, `Message`, `MessageRole`, `ChatRequest` imports.

- [ ] **Step 4: Check `cargo check`**

### Task 4.2: Update `orchestrator/planner.rs`

**File:** `src-tauri/src/orchestrator/planner.rs`

- [ ] **Step 1: Option A — Keep existing approach with Rig client**

If time-constrained, just replace the `provider.chat()` call with Rig agent:

```rust
// Before:
let response = provider.chat(request).await?;
let content = response.choices.first()...;

// After:
let agent = provider.agent(&mid)
    .preamble(&planner_system)
    .max_tokens(4096)
    .temperature(0.3)
    .build();
let content = agent.chat(&user_message, &mut vec![]).await?;
```

Keep the `extract_json()` + manual parsing because `ExecutionPlan` has complex structure.

- [ ] **Step 2: Option B — Use `extract_structured<ExecutionPlan>`** (preferred)

```rust
use crate::api::rig::extract_structured;

let plan: ExecutionPlan = extract_structured(
    &api_key,
    &model,
    &planner_system,
    &user_message,
).await.map_err(|e| ExecutionError::StepFailed { ... })?;
```

**Note:** Requires `OPENAI_API_KEY` env var or passing API key from config. If using the configured provider, Option A is simpler.

- [ ] **Step 3: Check `cargo check`**

### Task 4.3: Update `pipeline/engine.rs`

**File:** `src-tauri/src/pipeline/engine.rs`

- [ ] **Step 1: Update LLM call in `StepDef::LlmCall` handler**

```rust
// Before:
let provider = providers.get(mid)?;
let request = ChatRequest { messages, model, ... };
let response = provider.chat(request).await?;

// After:
let agent = provider.agent(&mid)
    .preamble(&sys.unwrap_or_default())
    .max_tokens(m_tokens.unwrap_or(4096) as u64)
    .temperature(temp.unwrap_or(0.7) as f64)
    .build();
let content = agent.chat(&prompt, &mut vec![]).await
    .map_err(|e| AppError::RigError(e.to_string()))?;
```

- [ ] **Step 2: Remove unused imports**

Remove `ChatRequest`, `Message`, `MessageRole` imports.

- [ ] **Step 3: Check `cargo check`**

### Task 4.4: Final integration check

- [ ] **Step 1: Full cargo check**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Clean build with zero errors

- [ ] **Step 2: Run existing tests**

```bash
cd src-tauri && cargo test 2>&1
```
Expected: All existing tests pass (or pre-existing failures documented)

- [ ] **Step 3: Build the full Tauri app**

```bash
cd src-tauri && cargo build 2>&1
```
Expected: Build succeeds

---

## Rollback Plan

If any phase introduces blocking issues:

1. Each phase is independently revert-able via `git checkout` on the affected files
2. If the new `ProviderRegistry` pattern breaks basic functionality, revert Phase 1 and keep `LLMProvider` trait (fallback)
3. If `ToolDyn` causes MCP integration issues, revert Phase 2 — keep custom `Tool` trait as a parallel implementation

## Verification Checklist

- [ ] Phase 1: `cargo check` passes with new provider types
- [ ] Phase 2: All 8 tools migrated, MCP tools working
- [ ] Phase 3: Agent loop, 3 workers, critic, classifier, summarizer use Rig directly
- [ ] Phase 4: Runtime, planner, pipeline use Rig directly
- [ ] `cargo test` passes (all existing tests)
- [ ] `cargo build` succeeds (full Tauri build)

---

*Plan generated from design doc: `docs/design/2026-05-29-rig-native-refactoring.md`*
