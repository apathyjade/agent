# Rig-native Refactoring Design

> **Date:** 2026-05-29
> **Status:** Draft
> **Target:** Replace all custom LLM abstractions with native Rig AI framework (v0.37) types and patterns.

## 1. Motivation & Goals

### Current Problem

The project wraps Rig behind two custom abstraction layers:

```
Custom types (api/types.rs) → LLMProvider trait (api/provider.rs) → RigProvider (api/rig.rs) → Rig client
```

This creates:
- **Duplicate type system**: `Message`, `ToolCall`, `ToolDefinition`, `ChatRequest`, `ChatResponse` all have custom versions that mirror Rig's native types
- **Ongoing conversion cost**: Every LLM call converts custom types → Rig types → provider format
- **Bypassed Rig features**: `rig::agent::Agent` multi-turn tool execution, `rig::tool::Tool`, `rig::ToolSet`, `rig::vector_store::InMemoryVectorStore`, `rig::tool::rmcp` are unused
- **Fragmented LLM paths**: Workers use `chat_text()` helper, Orchestrator uses `ChatRequest` directly, Planner uses `extract_structured` — all with different entry points

### Goals

1. Remove all custom LLM abstraction layers — Rig is the single entry point
2. Use Rig's native types (`rig::completion::Message`, `rig::completion::ToolDefinition`, etc.) directly for Tauri IPC (they already implement `Serialize`/`Deserialize`)
3. Replace `AgentLoop` with `rig::agent::Agent` for multi-turn tool execution
4. Replace custom `Tool` trait with `rig::tool::ToolDyn` for dynamic dispatch
5. Unify all LLM calling paths through Rig's `CompletionClient` or `Agent`
6. Leverage Rig's built-in `InMemoryVectorStore`, pipeline, and MCP modules where beneficial

## 2. Architecture

### Before

```
┌─────────────────────────────────────────────────────────┐
│                    Frontend (IPC)                        │
└────────────┬────────────────────────────────┬───────────┘
             │                                │
    ┌────────▼────────┐            ┌──────────▼──────────┐
    │ send_message/   │            │ execute_plan/       │
    │ send_message_   │            │ orchestrate_message │
    │ stream          │            │                     │
    └────────┬────────┘            └──────────┬──────────┘
             │                                │
    ┌────────▼────────┐            ┌──────────▼──────────┐
    │  IntentRouter   │            │  OrchestratorAgent  │
    │  → AgentLoop    │            │  → TaskGraph        │
    │  → LlmPlanner+  │            │  → Dispatcher       │
    │    Runtime      │            │  → CriticAgent      │
    └────────┬────────┘            └──────────┬──────────┘
             │                                │
             ▼                                ▼
    ┌──────────────────────────────────────────────────┐
    │           LLMProvider trait + chat_text()         │
    │           (Multiple call sites)                   │
    └───────────────────────┬──────────────────────────┘
                            │
                            ▼
                ┌───────────────────────┐
                │  RigProvider<C>       │
                │  (custom wrapper)     │
                └───────────┬───────────┘
                            │
                            ▼
                ┌───────────────────────┐
                │  rig::CompletionClient│
                │  (6-7 Rig providers)  │
                └───────────────────────┘
```

### After

```
┌────────────────────────────────────────────────────────┐
│                    Frontend (IPC)                       │
│  Uses rig::completion::Message, ToolCall, etc.          │
│  (all directly Serialize/Deserialize)                   │
└────────────┬───────────────────────────────┬───────────┘
             │                               │
    ┌────────▼────────┐           ┌──────────▼───────────┐
    │ send_message/   │           │ OrchestratorAgent    │
    │ send_message_   │           │ (DAG unchanged)      │
    │ stream          │           │                      │
    │                 │           │ Dispatcher → Workers │
    │ ToolLoop:       │           │ → CriticAgent        │
    │ rig::agent::    │           │ (all use Rig client) │
    │ Agent + ToolSet │           └──────────┬───────────┘
    └────────┬────────┘                      │
             │                               │
             ▼                               ▼
    ┌────────────────────────────────────────────────────┐
    │             rig::client::CompletionClient           │
    │             Direct - no intermediary trait           │
    │                                                      │
    │  provider.chat() → agent.chat() / agent.prompt()    │
    │  provider.agent() → builder.build() → Agent          │
    │  provider.extractor() → extract_structured()         │
    └───────────────────────┬────────────────────────────┘
                            │
                            ▼
                ┌──────────────────────────┐
                │  rig::providers::*::Client│
                │  (OpenAI, Anthropic, ...)  │
                └──────────────────────────┘
```

## 3. Module-by-Module Design

### 3.1 `api/types.rs` — Remove entire file

**Impact assessment:**
- Used by: IPC commands, AgentLoop, CriticAgent, LlmPlanner, PipelineEngine, Lifecycle, Intent classifier, DB models
- The DB layer uses its own `db::models::Message` for persistence — separate concern

**Migration path:**
1. Identify all `use crate::api::types::*` references
2. Replace with `use rig::completion::{Message, ToolCall, ToolDefinition, ...}`
3. `MessageRole` → `rig::completion::Message` variants handle role via enum discriminants
4. `ChatRequest` → built implicitly by `agent.chat()` / `agent.prompt()`
5. `ChatResponse` → returned by `agent.chat()` as `String`
6. `StreamPayload` → `MultiTurnStreamItem` / `StreamedAssistantContent`
7. `ToolDefinition` → `rig::completion::ToolDefinition` (identical fields)
8. `FunctionDefinition` → not needed (Rig wraps tool definitions differently)

**Key mapping:**

| Current (api/types.rs) | Rig native |
|------------------------|------------|
| `Message { role, content, tool_calls, tool_call_id }` | `rig::completion::Message::User(content)`, `::Assistant(vec![...])`, `::ToolResult(...)` |
| `MessageRole::System/User/Assistant/Tool` | `Message` enum variants |
| `ToolCall { id, name, arguments }` | `rig::completion::ToolCall { id, function: ToolFunction { name, arguments } }` |
| `ToolDefinition { tool_type, function }` | `rig::completion::ToolDefinition { name, description, parameters }` |
| `ChatRequest { messages, model, tools, stream, ... }` | `agent.chat(&msg)` / `agent.prompt()` / `agent.stream_chat()` |
| `ChatResponse { id, choices, usage }` | Method return values |
| `StreamPayload { content, tool_calls, finish_reason }` | `MultiTurnStreamItem` enum |

**DTO boundary consideration:**
- Tauri IPC return types need to be `Serialize`. Rig types already are.
- Frontend may expect different field naming. Use `#[serde(rename_all = "snake_case")]` where needed via newtype wrappers if fields differ.
- The `StreamEvent` enum in `agent/loop.rs` for frontend events needs to stay (it's an IPC-specific type for streaming progress).

### 3.2 `api/provider.rs` — Remove `LLMProvider` trait

**Current design:**
```rust
#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;
    async fn chat_stream(&self, request: ChatRequest) -> Result<BoxStream<'static, Result<StreamPayload>>>;
}
```

**New design:**
- `ProviderRegistry` directly wraps `Box<dyn CompletionClient + Send + Sync>`
- `get(model_id)` returns `Box<dyn CompletionClient + Send + Sync>`
- Factory `create_provider(model)` returns `Box<dyn CompletionClient>`
- `chat_text()` helper → callers use `provider.agent(&model).preamble().build().chat()` directly

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn CompletionClient + Send + Sync>>,
    default_model_id: Option<String>,
}
```

**Caller migration:**
- `agent.chat(request)` → `client.agent(&model).preamble(sys).build().chat(&msg, &mut history).await`
- `agent.chat_stream(request)` → `client.agent(&model).preamble(sys).build().stream_chat(&msg, &history).await`
- `chat_text(providers, model_id, system, user)` → inline `provider.agent().preamble().build().chat()` calls

### 3.3 `api/rig.rs` — Keep but simplify

**Before:** `RigProvider<C: CompletionClient>` generic wrapper implementing `LLMProvider`
**After:** Keep `create_rig_provider()` factory, `extract_structured<T>()`, remove `RigProvider` wrapper

```rust
// Factory returns Box<dyn CompletionClient>
pub fn create_rig_provider(model: &ModelConfig) -> Result<Box<dyn CompletionClient + Send + Sync>> {
    match model.provider {
        ModelProvider::OpenAI => {
            let client = rig::providers::openai::Client::new(&model.api_key)?;
            Ok(Box::new(client))
        }
        // ... other providers
    }
}

// Keep extract_structured as-is (already Rig-native)
pub async fn extract_structured<T>(...) -> Result<T> { ... }
```

**Type aliases removed** — consumers use `Box<dyn CompletionClient>` directly.

### 3.4 `agent/loop.rs` — Rewrite using `rig::agent::Agent`

**Before:**
- Manual tool-calling loop (LLM call → parse ToolCalls → execute → loop)
- Custom context optimization (`optimize_context` / `estimate_tokens`)
- Custom retry logic (`retry_with_backoff`)
- Custom stream event types (`StreamEvent`, `ToolCallInfo`, `ToolResultInfo`)

**After:**
- Internal use of `rig::agent::Agent` with `ToolSet` for multi-turn execution
- Keep `StreamEvent` IPC types (they're Tauri event formats for frontend)
- Remove `optimize_context` / `estimate_tokens` (Rig manages context internally):
  - *Note: Rig's agent may not have built-in context compression. If needed, keep a simplified version or use Rig's `Context` system.*
- Remove `retry_with_backoff` (Rig's HTTP client layer handles retries)

```rust
pub struct ToolLoop {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolSet>>,  // Use Rig's ToolSet
    max_iterations: usize,
}

impl ToolLoop {
    pub async fn run(&self, model_id: &str, messages: Vec<rig::completion::Message>, ...) -> Result<String> {
        let client = self.providers.lock().await.get(model_id)?;
        let tool_set = self.tools.lock().await.clone();
        
        let agent = client
            .agent(model_id)
            .preamble(system_prompt)
            .tool_set(tool_set)  // Rig's built-in tool handling
            .build();
        
        let response = agent.prompt(&user_message).await?;
        Ok(response)
    }
    
    // Stream variant uses agent.stream_chat() / agent.stream_prompt()
}
```

**Frontend event types (`StreamEvent`, `ToolCallInfo`, `ToolResultInfo`):**
Keep these as IPC-specific DTO types — they represent Tauri event payloads, not LLM types.

### 3.5 `tools/` — Adopt `rig::tool` system

#### 3.5.1 Custom `Tool` trait → `rig::tool::ToolDyn`

The current `Tool` trait uses `Arc<dyn Tool>` (dynamic dispatch). Rig's `Tool` trait uses generic associated types:

```rust
// Rig's Tool trait (static dispatch)
pub trait Tool: Sized {
    type Args: for<'a> Deserialize<'a>;
    type Output: Serialize;
    const NAME: &'static str;
    fn definition(&self) -> ToolDefinition;
    async fn call(&self, args: Self::Args) -> Result<Self::Output, ToolError>;
}
```

For our use case (heterogeneous collection via `HashMap<String, Arc<...>>`), use `ToolDyn`:

```rust
// Rig's ToolDyn trait (dynamic dispatch)
pub trait ToolDyn: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    async fn call_dyn(&self, args: Value) -> Result<Value, ToolError>;
}
```

**Each built-in tool** (calculator, file_system, web_search, code_executor, run_workflow, script_tool) implements `ToolDyn` instead of the custom `Tool` trait.

#### 3.5.2 `ToolRegistry` → Wrap Rig's `ToolSet` or keep lightweight

The registry currently provides:
- Name-based lookup (direct, alias, case-insensitive)
- Enable/disable state
- Dynamic registration (from MCP, skills)

**New design:** Keep `ToolRegistry` but internally store `HashMap<String, Arc<dyn ToolDyn>>`. Convert to Rig's `ToolSet` when building agents:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolDyn>>,
    enabled: HashMap<String, bool>,
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn to_rig_tool_set(&self) -> ToolSet {
        let mut builder = ToolSet::builder();
        for (name, tool) in self.tools.iter() {
            if self.enabled.get(name).copied().unwrap_or(false) {
                builder = builder.tool_dyn(tool.clone());
            }
        }
        builder.build()
    }
}
```

#### 3.5.3 Alias system

Keep the alias system in `ToolRegistry` — this is business logic, not LLM abstraction. The aliases allow flexible naming for the LLM to call tools by different names.

### 3.6 `mcp/manager.rs` — Adapt to `ToolDyn`

**`McpToolWrapper`** currently implements the custom `Tool` trait. Change to implement `ToolDyn`:

```rust
pub struct McpToolWrapper {
    name: String,
    description: String,
    parameters: Value,
    peer: Arc<Mutex<Peer>>,
}

#[async_trait]
impl ToolDyn for McpToolWrapper {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn parameters(&self) -> Value { self.parameters.clone() }
    async fn call_dyn(&self, args: Value) -> Result<Value, ToolError> {
        // Call peer.call_tool() as before
    }
}
```

**Future consideration:** `rig::tool::rmcp` module may provide direct MCP tool integration. Evaluate when upgrading Rig.

### 3.7 `workers/` — Use Rig agent for LLM calls

**Current pattern:**
```rust
// thinker.rs
let content = chat_text(&self.providers, model_id, system_prompt, &instruction, ...).await?;
```

**New pattern:**
```rust
let client = self.providers.lock().await.get(model_id)?;
let agent = client.agent(model_id)
    .preamble(system_prompt)
    .temperature(temperature)
    .build();
let response = agent.chat(&instruction, &mut vec![]).await?;
```

Each worker receives `Arc<Mutex<ProviderRegistry>>` and uses Rig's `CompletionClient` directly. This eliminates the `chat_text()` helper.

**Workers not affected by LLM change (no LLM calls):**
- `shell.rs` — direct command execution
- `web.rs` — HTTP fetch/search
- `memory.rs` — DB operations (uses Rig embeddings, which is already Rig-native)
- `mcp_bridge.rs` — MCP tool calls

### 3.8 `critic/agent.rs` — Simplify

**Before:** Uses `LLMProvider` trait via `ChatRequest` construction
**After:** Uses `CompletionClient` directly:

```rust
pub struct CriticAgent {
    providers: Arc<Mutex<ProviderRegistry>>,
}

impl CriticAgent {
    pub async fn review(&self, ...) -> Result<Critique> {
        let client = self.providers.lock().await.resolve(model_id)?;
        let agent = client.agent(model_id)
            .preamble(CRITIC_SYSTEM_PROMPT)
            .build();
        let response = agent.chat(&prompt, &mut vec![]).await?;
        // Parse JSON from response...
    }
}
```

### 3.9 `intent/classifier.rs` — Remove `llm_classify` fallback

**Current:** Three-tier approach (1) Rig `extract_structured` → (2) LLM chat → (3) keyword fallback
**After:** Single path — Rig `extract_structured`. The `llm_classify()` fallback exists because the original needed `OPENAI_API_KEY` env var. After refactoring, extractor should use any configured provider.

```rust
pub async fn classify(&self, message: &str) -> ClassificationResult {
    match self.rig_classify(message).await {
        Ok(result) => result,
        Err(e) => {
            log::warn!("Rig classify failed: {}", e);
            fallback_classify(message) // Keep keyword fallback for offline
        }
    }
}
```

### 3.10 `lifecycle/summarizer.rs` — Simplify

Replace `ChatRequest` construction with direct `CompletionClient` usage.

### 3.11 `orchestrator/runtime.rs` — Use Rig client

**Before:**
```rust
let provider = providers.lock().await.get(mid)?;
let request = ChatRequest { messages, model, tools, ... };
let response = provider.chat(request).await?;
```

**After:**
```rust
let provider = providers.lock().await.get(mid)?;
let agent = provider.agent(&mid)
    .preamble(system_prompt)
    .build();
let response = agent.chat(&user_message, &mut history).await?;
```

### 3.12 `pipeline/engine.rs` — Simplify

Same pattern as runtime — replace `ChatRequest` with `CompletionClient.agent().chat()`.

### 3.13 `orchestrator/planner.rs` — Use `extract_structured`

The planner already parses JSON from LLM responses. Replace with Rig's `extract_structured<ExecutionPlan>` for type-safe extraction. This eliminates the manual JSON parsing and validation logic.

### 3.14 `memory/vector_index.rs` — Optional replacement

**Current:** Custom `InMemoryVectorIndex` with brute-force cosine similarity.
**Available:** `rig::vector_store::InMemoryVectorStore` with similar functionality.

Evaluate if the custom implementation provides features not in Rig's `InMemoryVectorStore` (e.g., SQLite persistence integration). If not, migrate to Rig's version.

## 4. Type Migration Matrix

### IPC Types (Tauri commands → Frontend)

| Current IPC type | Replacement | Status |
|---|---|---|
| `commands::StreamChunk` | Keep (Tauri-specific format) | ✅ Keep |
| `commands::ToolCallEvent` | Keep (Tauri-specific format) | ✅ Keep |
| `agent::loop::StreamEvent` | Keep (Tauri event enum) | ✅ Keep |
| `tools::trait::ToolInfo` | Keep (metadata for frontend) | ✅ Keep |
| `api::types::Message` | `rig::completion::Message` | 🔄 Replace |
| `api::types::ToolCall` | `rig::completion::ToolCall` | 🔄 Replace |
| `api::types::ToolDefinition` | `rig::completion::ToolDefinition` | 🔄 Replace |

### Internal LLM Types

| Current type | Replacement | Migration |
|---|---|---|
| `ChatRequest` | Inline params in Rig API calls | Remove |
| `ChatResponse` | `String` from `agent.chat()` | Remove |
| `StreamPayload` | `MultiTurnStreamItem`/`StreamedAssistantContent` | Remove |
| `FunctionDefinition` | `rig::completion::ToolDefinition` | Remove |

### DB Types (separate concern)

The `db::models::Message` and related types are DB-specific and remain unchanged. They serialize/deserialize from SQLite independently of Rig types.

## 5. IPC Compatibility

### Frontend expectations

Current frontend receives:
- `Message { id, role, content, tool_calls, tool_call_id }`
- `ToolCall { id, name, arguments }`
- `StreamPayload { content, tool_calls, finish_reason }`

Rig counterparts:
- `rig::completion::Message` is an enum (not a struct with `role` field)
- `rig::completion::ToolCall { id, function: ToolFunction { name, arguments } }` (nested vs flat `name`)

**Solution:** Keep IPC DTO layer for command return types where the shape differs. These are serialize-only types defined in `commands/` module:

```rust
// In commands/session.rs — minimal DTO for frontend compatibility
#[derive(Serialize)]
pub struct IpcMessage {
    pub id: Option<String>,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<IpcToolCall>>,
    pub tool_call_id: Option<String>,
}

impl From<rig::completion::Message> for IpcMessage { ... }
```

**Not all Rig types need DTOs** — only where Tauri IPC serialization shape differs. Internal code uses Rig types exclusively.

## 6. Migration Phases

### Phase 1: Core types + Provider layer

**Files to change:**
- `api/types.rs` — Remove / gut
- `api/provider.rs` — Remove `LLMProvider` trait, change `ProviderRegistry` to store `Box<dyn CompletionClient>`
- `api/rig.rs` — Remove `RigProvider<C>`, keep factory + extractor
- `config.rs` — No change needed
- `error.rs` — Update `AppError::Provider` to wrap `rig::error::Error`

**Verification:** `cargo check` passes, provider creation works.

### Phase 2: Tool system

**Files to change:**
- `tools/trait.rs` — Change to use `rig::tool::ToolDyn`
- `tools/registry.rs` — Update to store `Arc<dyn ToolDyn>`, add `to_rig_tool_set()`
- `tools/calculator.rs` — Implement `ToolDyn`
- `tools/file_system.rs` — Implement `ToolDyn`
- `tools/web_search.rs` — Implement `ToolDyn`
- `tools/code_executor.rs` — Implement `ToolDyn`
- `tools/run_workflow.rs` — Implement `ToolDyn`
- `tools/script_tool.rs` — Implement `ToolDyn`
- `mcp/manager.rs` — `McpToolWrapper` implements `ToolDyn`
- `mcp/bridge.rs` — `McpToolBridge` implements `ToolDyn`
- `skills/mod.rs` — Update tool registration

**Verification:** `cargo check` passes, tools are registered and callable.

### Phase 3: Agent Loop + LLM call sites

**Files to change:**
- `agent/loop.rs` — Rewrite using Rig Agent
- `workers/thinker.rs` — Direct Rig client call
- `workers/code_explorer.rs` — Direct Rig client call
- `workers/code_editor.rs` — Direct Rig client call
- `critic/agent.rs` — Direct Rig client call
- `intent/classifier.rs` — Unify extractor path
- `lifecycle/summarizer.rs` — Direct Rig client call

**Verification:** `cargo check` passes, agent loop runs tools correctly.

### Phase 4: Orchestrator + Pipeline

**Files to change:**
- `orchestrator/runtime.rs` — Direct Rig client calls
- `orchestrator/planner.rs` — Use `extract_structured`
- `pipeline/engine.rs` — Direct Rig client calls
- `orchestrator/agent.rs` — Update CriticAgent usage

**Verification:** Full integration test passes.

## 7. Error Handling

### Error Mapping

| Current Error | Rig Error | Action |
|---|---|---|
| `AppError::Provider(msg)` | `rig::error::RequestError` | Wrap in `AppError::Provider` |
| `AppError::Tool(msg)` | `rig::tool::ToolError` | Keep custom, add ToolError source |
| `AppError::Orchestrator(msg)` | N/A | Keep as-is |

The `AppError` enum gains a `RigError` variant for unhandled Rig errors:

```rust
pub enum AppError {
    // ... existing variants
    RigError(String),  // New: wraps rig errors not fitting other categories
}
```

## 8. Testing Strategy

- **Unit tests**: Existing tests for Tool implementations, AgentLoop logic, CriticAgent rules remain valid (they test business logic, not LLM plumbing)
- **Integration tests**: Update test helpers that create `ProviderRegistry`/`AgentLoop` to use new APIs
- **E2E tests**: Frontend IPC path unchanged — same commands, potentially different message shapes

### Tests that need updating

| Test location | Change |
|---|---|
| `agent/loop.rs` tests | Update `AgentLoop` construction, `optimize_context` removed |
| `tools/registry.rs` tests | Update trait references |
| `tools/calculator.rs` tests (if any) | Update trait impl |
| `critic/agent.rs` tests | Update `CriticAgent` construction |
| `orchestrator/agent.rs` tests | Update `Dispatcher`/workers |
| `orchestrator/runtime.rs` tests | Update LLM call helpers |

## 9. Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| **Rig `Message` enum differs from our struct** | Frontend compatibility | Keep IPC DTO layer for shape differences |
| **Rig Agent tool execution may behave differently** | Multi-turn tool handling | Test with existing tool-heavy scenarios |
| **Rig context management may not replace custom compression** | Context window overflow | Keep simplified context optimization if Rig lacks compression |
| **Performance regression in tool loop** | User experience | Benchmark before/after, optimize Runtime |
| **DB models depend on custom types** | Build failure | DB uses its own `db::models` types (already separate) |

## 10. Future Opportunities

After this refactoring, the project can leverage additional Rig features:

- **`rig::pipeline`** — Replace custom YAML pipeline engine with Rig's pipeline system
- **`rig::vector_store::InMemoryVectorStore`** — Replace custom vector index
- **`rig::tool::rmcp`** — Replace custom rmcp integration
- **`rig::vector_store::LanceDbVectorStore`** — Scalable persistent vector search
- **Rig `Context` system** — Standardized context management

---

*Design reviewed and approved. Transitioning to implementation planning.*
