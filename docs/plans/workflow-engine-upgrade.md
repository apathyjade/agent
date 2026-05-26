# Phase 1 实施方案：工作流引擎升级

> 基于 PRD V2 模块 B | 委托 Sisyphus 执行

---

## TL;DR

升级工作流系统 4 项核心能力：Cron 定时触发、LLM 真实调用、步骤时间线、错误处理。

**变更文件**：~8 个 Rust 文件 + ~3 个前端文件，预计净增 400-500 行。

---

## 任务 1：添加依赖 + 升级数据模型

### 1.1 Cargo.toml 添加依赖

**文件**：`src-tauri/Cargo.toml`

在 `rmcp` 行后添加：
```toml
tokio-cron-scheduler = "1"
```

### 1.2 升级 WorkflowDef 模型

**文件**：`src-tauri/src/pipeline/models.rs`

**改动**：完整重写，添加新类型。

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 触发器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDef {
    Manual,
    Cron { schedule: String },
    FileWatch { path: String, pattern: String },
}

/// 工作流定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub trigger: TriggerDef,
    #[serde(default)]
    pub steps: Vec<StepDef>,
}

/// 单步定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDef {
    ToolCall {
        id: String,
        tool: String,
        #[serde(default)]
        params: HashMap<String, Value>,
        #[serde(default)]
        retry: Option<RetryConfig>,
        #[serde(default = "default_on_error")]
        on_error: String,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    LlmCall {
        id: String,
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        model_id: Option<String>,
        #[serde(default)]
        system_prompt: Option<String>,
        #[serde(default)]
        max_tokens: Option<u32>,
        #[serde(default)]
        temperature: Option<f32>,
        #[serde(default)]
        retry: Option<RetryConfig>,
        #[serde(default = "default_on_error")]
        on_error: String,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    Condition {
        id: String,
        condition: String,
        #[serde(default = "default_on_false")]
        on_false: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max: u32,
    #[serde(default = "default_retry_delay")]
    pub delay_seconds: u64,
}

fn default_retry_delay() -> u64 { 3 }
fn default_on_error() -> String { "fail".to_string() }
fn default_on_false() -> String { "end".to_string() }

// 运行时结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepProgress {
    pub step_id: String,
    pub status: String,  // pending|running|completed|failed|skipped
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub id: String,
    pub workflow_name: String,
    pub status: String,
    pub step_results: Option<String>,
    pub step_progress: Option<String>,  // JSON Vec<StepProgress>
    pub error: Option<String>,
    pub trigger_type: String,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    pub name: String,
    pub description: String,
    pub step_count: usize,
    pub file_path: String,
    pub trigger: String,  // "manual" | "cron: 0 9 * * *" | "file: ~/Downloads/*"
    pub next_run_at: Option<String>,
    pub last_run_status: Option<String>,
    pub last_run_at: Option<String>,
}

impl WorkflowDef {
    pub fn render_template(template: &str, step_results: &HashMap<String, Value>) -> String {
        let mut result = template.to_string();
        for (step_id, value) in step_results {
            let placeholder = format!("{{{{ steps.{}.result }}}}", step_id);
            let rendered = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &rendered);
        }
        result
    }
}
```

**重点**：
- 新增 `TriggerDef` 枚举（Manual/Cron/FileWatch）
- `StepDef` 变体增加 `retry`、`on_error`、`timeout_seconds`
- `LlmCall` 增加 `system_prompt`、`max_tokens`、`temperature`
- 新增 `StepProgress` 运行时结构
- `WorkflowRecord` 增加 `step_progress`、`trigger_type`
- `WorkflowInfo` 增加 `trigger`、`next_run_at`

---

## 任务 2：重写 PipelineEngine

**文件**：`src-tauri/src/pipeline/engine.rs`（完整重写）

### 核心改动

```
旧引擎：PipelineEngine { tools, db }
新引擎：PipelineEngine { tools, db, providers, event_tx }
       + 新增异步事件发送 channel
       + 新增 Cron 调度器 handle
```

### 新增 imports

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::api::provider::ProviderRegistry;
use crate::api::types::{ChatRequest, Message, MessageRole};
use crate::pipeline::models::{StepDef, StepProgress, TriggerDef, WorkflowDef, WorkflowRunRecord};
use crate::tools::registry::ToolRegistry;
```

### PipelineEngine 新增字段

```rust
pub struct PipelineEngine {
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<crate::db::repository::Database>>,
    providers: Arc<Mutex<ProviderRegistry>>,
    event_tx: mpsc::Sender<StepProgress>,
    scheduler: Arc<Mutex<Option<JobScheduler>>>,
}
```

### 关键方法

**1. `new()` — 新增 providers 参数**
```rust
pub fn new(
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<crate::db::repository::Database>>,
    providers: Arc<Mutex<ProviderRegistry>>,
    event_tx: mpsc::Sender<StepProgress>,
) -> Self
```

**2. `run()` — 升级核心循环**

每个步骤执行时：
- 发送 `StepProgress { status: "running" }` 到 event_tx
- 获取开始时间戳
- 执行（含 retry 逻辑）
- 计算耗时，发送 `StepProgress { status: "completed", duration_ms }` 
- 如果失败且 `on_error == "skip"`，跳过继续
- 如果失败且 `on_error == "fail"`，终止

**3. LLM 步骤真实调用**

```rust
StepDef::LlmCall { id, prompt, model_id, system_prompt, max_tokens, temperature, .. } => {
    let providers = self.providers.lock().await;
    
    // 用默认模型或指定模型
    let mid = model_id.as_deref().unwrap_or_else(|| {
        providers.default_model_id()
    });
    
    let provider = providers.get(mid)?;
    let rendered_prompt = WorkflowDef::render_template(prompt, &step_results);
    
    let request = ChatRequest {
        messages: vec![
            Message {
                id: None,
                role: MessageRole::System,
                content: system_prompt.clone().unwrap_or_default(),
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                id: None,
                role: MessageRole::User,
                content: rendered_prompt,
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        model: mid.to_string(),
        tools: None,
        stream: Some(false),
    };
    
    let response = provider.chat(request).await?;
    let content = response.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    step_results.insert(id.clone(), Value::String(content));
}
```

**4. Retry 逻辑封装**

```rust
async fn execute_with_retry<F, Fut>(
    &self,
    step_id: &str,
    retry: &Option<RetryConfig>,
    on_error: &str,
    timeout_seconds: Option<u64>,
    f: F,
) -> Result<Value>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let max = retry.as_ref().map(|r| r.max).unwrap_or(1);
    let delay = retry.as_ref().map(|r| r.delay_seconds).unwrap_or(0);
    let mut last_err = None;
    
    for attempt in 0..max {
        let fut = f();
        let result = if let Some(timeout) = timeout_seconds {
            match tokio::time::timeout(Duration::from_secs(timeout), fut).await {
                Ok(r) => r,
                Err(_) => Err(AppError::Tool(format!("Step '{}' timed out", step_id))),
            }
        } else {
            fut.await
        };
        
        match result {
            Ok(val) => return Ok(val),
            Err(e) => {
                last_err = Some(e);
                if attempt < max - 1 && delay > 0 {
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
            }
        }
    }
    
    match on_error {
        "skip" => Ok(Value::Null),
        _ => Err(last_err.unwrap()),
    }
}
```

**5. Cron 调度器启动方法**

```rust
pub async fn start_cron_jobs(
    &self,
    workflows: Vec<(PathBuf, WorkflowDef)>,
    engine: Arc<Self>,
    event_tx: mpsc::Sender<StepProgress>,
) -> Result<JobScheduler>
```

每 30 秒检查一次工作流文件变化，自动重启调度器。

---

## 任务 3：升级 scanner + PipelineEngine 集成

### scanner.rs 升级

**文件**：`src-tauri/src/pipeline/scanner.rs`

`scan_workflow_files()` 返回 `Vec<(PathBuf, WorkflowDef)>` 不变。
`list_workflows()` 新增 `next_run_at` 计算。

---

## 任务 4：IPC 命令升级

### 升级 pipeline 命令

**文件**：`src-tauri/src/commands/pipeline.rs`

新增命令：
- `pause_workflow_schedule(name:string) -> ()`
- `resume_workflow_schedule(name:string) -> ()`
- `get_workflow_run_detail(id:string) -> WorkflowRunRecord`（含 step_progress）

升级 `run_workflow`：返回运行 ID，前端可轮询进度。

---

## 任务 5：DB 升级

### 升级 workflow_runs 表

**文件**：`src-tauri/src/db/repository.rs`

ALTER TABLE 添加 `trigger_type TEXT NOT NULL DEFAULT 'manual'` 和 `step_progress TEXT` 列（或用新的 migration 函数）。

---

## 任务 6：AppState 集成

### state.rs 升级

**文件**：`src-tauri/src/state.rs`

在 `AppState` 中新增：
```rust
pub pipeline_engine: Arc<PipelineEngine>,
pub step_progress_tx: mpsc::Sender<StepProgress>,
```

在 `lib.rs` 启动时初始化 engine 并调用 `start_cron_jobs()`。

---

## 任务 7：前端升级

### 7.1 类型升级

**文件**：`src-ui/src/types/index.ts`

```typescript
export interface StepProgress {
  step_id: string;
  status: string;
  duration_ms?: number | null;
  error?: string | null;
  result_summary?: string | null;
}

export interface WorkflowRunRecord {
  id: string;
  workflow_name: string;
  status: string;
  step_results?: string | null;
  step_progress?: string | null;  // JSON StepProgress[]
  trigger_type: string;
  error?: string | null;
  started_at: string;
  finished_at?: string | null;
}

export interface WorkflowInfo {
  name: string;
  description: string;
  step_count: number;
  file_path: string;
  trigger: string;
  next_run_at?: string | null;
  last_run_status?: string | null;
  last_run_at?: string | null;
}
```

### 7.2 API 封装升级

**文件**：`src-ui/src/api/tauri.ts`

新增：
```typescript
export async function pauseWorkflowSchedule(name: string): Promise<void>
export async function resumeWorkflowSchedule(name: string): Promise<void>
export async function getWorkflowRunDetail(id: string): Promise<WorkflowRunRecord>
```

### 7.3 前端工作流选项卡升级

**文件**：`src-ui/src/components/SettingsModal.tsx`（工作流标签页）

当前是静态列表。新增：
- 触发器类型标签（⏰ 定时 / 🖱️ 手动）
- 下次执行时间显示
- 暂停/恢复按钮
- 运行后实时显示步骤进度（轮询 `get_workflow_run_detail`）

---

## 任务 8：构建验证

```bash
cd src-tauri && cargo check
cd src-ui && npx tsc --noEmit && npx vite build
```

---

## 验收样例

### 定时工作流 YAML

在 `~/.config/agent/workflows/` 创建 `hourly-check.yaml`：

```yaml
name: "Hourly System Check"
description: "每小时检查系统状态"
trigger:
  type: cron
  schedule: "0 * * * *"
steps:
  - id: list_files
    type: tool_call
    tool: file_system
    params:
      action: list
      path: "~/Downloads"
  
  - id: analyze
    type: llm_call
    prompt: |
      以下是 Downloads 目录的内容：{{ steps.list_files.result }}
      简要概括有哪些新文件（用中文回复）
    retry:
      max: 2
      delay_seconds: 5
    on_error: skip
```

---

## 变更文件清单

| 文件 | 操作 | 行数变化 |
|------|------|----------|
| `src-tauri/Cargo.toml` | 添加 dep | +1 |
| `src-tauri/src/pipeline/models.rs` | 重写 | ~100 |
| `src-tauri/src/pipeline/engine.rs` | 重写 | ~150 |
| `src-tauri/src/pipeline/scanner.rs` | 升级 | ~20 |
| `src-tauri/src/commands/pipeline.rs` | 新增命令 | ~30 |
| `src-tauri/src/state.rs` | 新增字段 | ~10 |
| `src-tauri/src/lib.rs` | 启动集成 | ~15 |
| `src-tauri/src/db/repository.rs` | migration + 新方法 | ~30 |
| `src-ui/src/types/index.ts` | 新增类型 | ~30 |
| `src-ui/src/store/workflowSlice.ts` | 新增方法 | ~20 |
| `src-ui/src/api/tauri.ts` | 新增 API | ~10 |
| `src-ui/src/components/SettingsModal.tsx` | 升级 UI | ~50 |
| | **总计** | **~470 行净增** |
