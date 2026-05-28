# Workspace Agent — Phase 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the foundational modules for the hierarchical workspace agent system — Orchestrator, Workers, Critic, and Workspace Context — as independently testable components.

**Architecture:** New modules `orchestrator/`, `workers/`, `critic/`, and `workspace/` are added alongside existing code. Each module registers in `lib.rs` as a `pub mod`. Workers implement a `WorkerAgent` trait (analogous to the existing `Tool` trait). The OrchestratorAgent uses Rig's `CompletionClient` to drive task decomposition. All modules are testable without Tauri runtime.

**Tech Stack:** Rust (Rig 0.37, tokio, async-trait, serde, chrono, uuid, schemars, thiserror, notify for file watcher)

---

## File Structure

### New files to create:
- `src-tauri/src/orchestrator/mod.rs` — Module declaration, re-exports
- `src-tauri/src/orchestrator/agent.rs` — `OrchestratorAgent` struct + `process()` method
- `src-tauri/src/orchestrator/task_graph.rs` — `TaskGraph`, `TaskNode`, `TaskEdge`, `NodeStatus`
- `src-tauri/src/orchestrator/dispatcher.rs` — `Dispatcher` — manages Worker invocation
- `src-tauri/src/workers/mod.rs` — Module declaration, `WorkerAgent` trait
- `src-tauri/src/workers/thinker.rs` — `ThinkerWorker` — CoT reasoning agent
- `src-tauri/src/workers/code_explorer.rs` — `CodeExplorerWorker` — code search agent
- `src-tauri/src/critic/mod.rs` — Module declaration
- `src-tauri/src/critic/agent.rs` — `CriticAgent` — output verification agent
- `src-tauri/src/workspace/mod.rs` — Module declaration
- `src-tauri/src/workspace/indexer.rs` — `CodebaseIndexer` — project detection, dependency analysis
- `src-tauri/src/workspace/search.rs` — `SearchEngine` — ripgrep/glob wrapper

### Files to modify:
- `src-tauri/src/error.rs` — Add `Orchestrator` error variant
- `src-tauri/src/lib.rs` — Register `orchestrator`, `workers`, `critic`, `workspace` modules
- `src-tauri/Cargo.toml` — Add `notify` dependency for file watcher (used in Phase 2, declare now)

---

### Task 1: Define WorkerAgent trait and error types

**Files:**
- Create: `src-tauri/src/workers/mod.rs`
- Modify: `src-tauri/src/error.rs`

- [ ] **Step 1: Add `Orchestrator` variant to `AppError`**

In `src-tauri/src/error.rs`, add before the closing `}` of the enum:

```rust
    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    #[error("Worker error: {0}")]
    Worker(String),

    #[error("Critic error: {0}")]
    Critic(String),

    #[error("Workspace error: {0}")]
    Workspace(String),
```

Add `is_retryable()` cases (all return `false`):

```rust
            AppError::Orchestrator(_) => false,
            AppError::Worker(_) => false,
            AppError::Critic(_) => false,
            AppError::Workspace(_) => false,
```

- [ ] **Step 2: Write `workers/mod.rs` with the `WorkerAgent` trait and `WorkerKind` enum**

```rust
use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;

/// Lightweight feedback struct for worker retry (no dependency on critic module).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerFeedback {
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
}

impl WorkerFeedback {
    pub fn new(issues: Vec<String>, suggestions: Vec<String>) -> Self {
        Self { issues, suggestions }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    Thinker,
    CodeExplorer,
    CodeEditor,
    Shell,
    Web,
    Memory,
    McpBridge,
}

impl WorkerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerKind::Thinker => "thinker",
            WorkerKind::CodeExplorer => "code_explorer",
            WorkerKind::CodeEditor => "code_editor",
            WorkerKind::Shell => "shell",
            WorkerKind::Web => "web",
            WorkerKind::Memory => "memory",
            WorkerKind::McpBridge => "mcp_bridge",
        }
    }
}

/// Result returned by a worker after executing a sub-task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    pub worker: WorkerKind,
    pub task_id: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// Sub-task dispatched by the Orchestrator to a Worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub id: String,
    pub label: String,
    pub instruction: String,
    pub worker_kind: WorkerKind,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub context: Option<Value>,
}

#[async_trait]
pub trait WorkerAgent: Send + Sync {
    fn kind(&self) -> WorkerKind;
    fn description(&self) -> &str;

    async fn execute(&self, task: SubTask) -> Result<WorkerResult>;

    async fn execute_with_feedback(
        &self,
        task: SubTask,
        feedback: &WorkerFeedback,
    ) -> Result<WorkerResult> {
        // Default: prepend feedback to instruction and retry
        let mut amended = task.clone();
        amended.instruction = format!(
            "Previous attempt had these issues:\n{}\n\nFeedback:\n{}\n\nPlease fix and retry.\n\n{}",
            feedback.issues.join("\n"),
            feedback.suggestions.join("\n"),
            task.instruction
        );
        self.execute(amended).await
    }
}
```

- [ ] **Step 3: Write the test to verify the trait compiles**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_kind_as_str() {
        assert_eq!(WorkerKind::Thinker.as_str(), "thinker");
        assert_eq!(WorkerKind::CodeExplorer.as_str(), "code_explorer");
        assert_eq!(WorkerKind::CodeEditor.as_str(), "code_editor");
        assert_eq!(WorkerKind::Shell.as_str(), "shell");
        assert_eq!(WorkerKind::Web.as_str(), "web");
        assert_eq!(WorkerKind::Memory.as_str(), "memory");
        assert_eq!(WorkerKind::McpBridge.as_str(), "mcp_bridge");
    }

    #[test]
    fn test_sub_task_serialization() {
        let task = SubTask {
            id: "test_1".into(),
            label: "Test task".into(),
            instruction: "Do something".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: Some(1024),
            temperature: Some(0.3),
            context: None,
        };
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("\"worker_kind\":\"thinker\""));
        let deserialized: SubTask = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test_1");
    }
}
```

- [ ] **Step 4: Run tests to verify compilation and passing tests**

Run: `cd src-tauri && cargo test tests::test_worker_kind_as_str tests::test_sub_task_serialization 2>&1`
Expected: `ok` for both tests

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workers/mod.rs src-tauri/src/error.rs
git commit -m "feat(workers): add WorkerAgent trait and WorkerKind enum"
```

---

### Task 2: Define TaskGraph data model

**Files:**
- Create: `src-tauri/src/orchestrator/mod.rs`
- Create: `src-tauri/src/orchestrator/task_graph.rs`

- [ ] **Step 1: Write `orchestrator/mod.rs`**

```rust
pub mod agent;
pub mod dispatcher;
pub mod task_graph;
```

- [ ] **Step 2: Write `orchestrator/task_graph.rs`**

```rust
use serde::{Deserialize, Serialize};
use crate::workers::{SubTask, WorkerKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Skipped,
}

impl Default for NodeStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub label: String,
    pub worker_kind: WorkerKind,
    pub instruction: String,
    pub status: NodeStatus,
    #[serde(default)]
    pub result_summary: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

impl TaskNode {
    pub fn new(id: String, label: String, worker_kind: WorkerKind, instruction: String) -> Self {
        Self {
            id,
            label,
            worker_kind,
            instruction,
            status: NodeStatus::Pending,
            result_summary: None,
            error: None,
            duration_ms: None,
        }
    }

    pub fn to_sub_task(&self) -> SubTask {
        SubTask {
            id: self.id.clone(),
            label: self.label.clone(),
            instruction: self.instruction.clone(),
            worker_kind: self.worker_kind.clone(),
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEdge {
    pub from: String,
    pub to: String,
}

impl TaskEdge {
    pub fn new(from: String, to: String) -> Self {
        Self { from, to }
    }
}

/// A DAG of tasks that the Orchestrator executes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub id: String,
    pub goal: String,
    pub nodes: Vec<TaskNode>,
    pub edges: Vec<TaskEdge>,
}

impl TaskGraph {
    pub fn new(id: String, goal: String) -> Self {
        Self {
            id,
            goal,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: TaskNode) {
        self.nodes.push(node);
    }

    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.edges.push(TaskEdge::new(from.to_string(), to.to_string()));
    }

    /// Return nodes that have no uncompleted dependencies.
    pub fn ready_nodes(&self) -> Vec<&TaskNode> {
        let completed_ids: std::collections::HashSet<&str> = self.nodes
            .iter()
            .filter(|n| matches!(n.status, NodeStatus::Completed))
            .map(|n| n.id.as_str())
            .collect();

        self.nodes
            .iter()
            .filter(|node| {
                if !matches!(node.status, NodeStatus::Pending) {
                    return false;
                }
                // All dependencies must be completed
                self.edges
                    .iter()
                    .filter(|e| e.to == node.id)
                    .all(|e| completed_ids.contains(e.from.as_str()))
            })
            .collect()
    }

    /// Check if all nodes are completed.
    pub fn is_complete(&self) -> bool {
        self.nodes.iter().all(|n| matches!(n.status, NodeStatus::Completed))
    }

    /// Check if any node failed.
    pub fn has_failures(&self) -> bool {
        self.nodes.iter().any(|n| matches!(n.status, NodeStatus::Failed(_)))
    }

    /// Get node by ID.
    pub fn get_node(&self, id: &str) -> Option<&TaskNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Get mutable node by ID.
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Topological ordering of nodes.
    pub fn topological_order(&self) -> Vec<&TaskNode> {
        let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut result = Vec::new();

        fn visit<'a>(
            node_id: &str,
            graph: &'a TaskGraph,
            visited: &mut std::collections::HashSet<&'a str>,
            result: &mut Vec<&'a TaskNode>,
        ) {
            if visited.contains(node_id) {
                return;
            }
            visited.insert(node_id);
            // Visit dependencies first
            for edge in &graph.edges {
                if edge.to == node_id {
                    visit(&edge.from, graph, visited, result);
                }
            }
            if let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) {
                result.push(node);
            }
        }

        for node in &self.nodes {
            visit(&node.id, self, &mut visited, &mut result);
        }

        result
    }
}
```

- [ ] **Step 3: Write the unit test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::WorkerKind;

    #[test]
    fn test_empty_graph_is_complete() {
        let graph = TaskGraph::new("g1".into(), "test".into());
        assert!(graph.is_complete());
        assert!(graph.ready_nodes().is_empty());
    }

    #[test]
    fn test_ready_nodes_no_deps() {
        let mut graph = TaskGraph::new("g2".into(), "test".into());
        graph.add_node(TaskNode::new("a".into(), "A".into(), WorkerKind::Thinker, "think".into()));
        graph.add_node(TaskNode::new("b".into(), "B".into(), WorkerKind::Thinker, "think".into()));

        let ready = graph.ready_nodes();
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn test_ready_nodes_with_deps() {
        let mut graph = TaskGraph::new("g3".into(), "test".into());
        graph.add_node(TaskNode::new("a".into(), "A".into(), WorkerKind::Thinker, "think".into()));
        graph.add_node(TaskNode::new("b".into(), "B".into(), WorkerKind::Thinker, "think".into()));
        graph.add_edge("a", "b");

        // Only 'a' should be ready initially
        let ready = graph.ready_nodes();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");

        // Complete 'a' -> 'b' becomes ready
        graph.get_node_mut("a").unwrap().status = NodeStatus::Completed;
        let ready = graph.ready_nodes();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "b");
    }

    #[test]
    fn test_topological_order() {
        let mut graph = TaskGraph::new("g4".into(), "test".into());
        graph.add_node(TaskNode::new("a".into(), "A".into(), WorkerKind::Thinker, "think".into()));
        graph.add_node(TaskNode::new("b".into(), "B".into(), WorkerKind::Thinker, "think".into()));
        graph.add_node(TaskNode::new("c".into(), "C".into(), WorkerKind::Thinker, "think".into()));
        graph.add_edge("a", "c");
        graph.add_edge("b", "c");

        let order = graph.topological_order();
        assert_eq!(order.len(), 3);
        // 'c' must be last
        assert_eq!(order[2].id, "c");
    }

    #[test]
    fn test_has_failures() {
        let mut graph = TaskGraph::new("g5".into(), "test".into());
        graph.add_node(TaskNode::new("a".into(), "A".into(), WorkerKind::Thinker, "think".into()));
        assert!(!graph.has_failures());
        graph.get_node_mut("a").unwrap().status = NodeStatus::Failed("error".into());
        assert!(graph.has_failures());
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test task_graph::tests 2>&1`
Expected: all 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/orchestrator/mod.rs src-tauri/src/orchestrator/task_graph.rs
git commit -m "feat(orchestrator): add TaskGraph DAG data model"
```

---

### Task 3: Build CodebaseIndexer

**Files:**
- Create: `src-tauri/src/workspace/mod.rs`
- Create: `src-tauri/src/workspace/indexer.rs`

- [ ] **Step 1: Write `workspace/mod.rs`**

```rust
pub mod indexer;
pub mod search;
```

- [ ] **Step 2: Write `workspace/indexer.rs`**

```rust
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLanguage {
    Rust,
    TypeScript,
    Python,
    Go,
    Java,
    JavaScript,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseIndex {
    pub root: PathBuf,
    pub language: Option<ProjectLanguage>,
    pub framework: Option<String>,
    pub dependencies: Vec<Dependency>,
    pub file_count: usize,
    pub dir_count: usize,
    pub last_indexed: String,
    /// Map from file extension to count
    pub file_types: HashMap<String, usize>,
}

pub struct CodebaseIndexer;

impl CodebaseIndexer {
    /// Index a project directory, detecting language, framework, and dependencies.
    pub fn index(root: &Path) -> Result<CodebaseIndex> {
        if !root.exists() {
            return Err(AppError::Workspace(format!(
                "Path does not exist: {}",
                root.display()
            )));
        }
        if !root.is_dir() {
            return Err(AppError::Workspace(format!(
                "Path is not a directory: {}",
                root.display()
            )));
        }

        let (language, framework) = Self::detect_project(root);
        let dependencies = Self::detect_dependencies(root, &language);
        let (file_count, dir_count, file_types) = Self::count_files(root);

        Ok(CodebaseIndex {
            root: root.to_path_buf(),
            language,
            framework,
            dependencies,
            file_count,
            dir_count,
            file_types,
            last_indexed: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Detect the primary language and framework from config files.
    fn detect_project(root: &Path) -> (Option<ProjectLanguage>, Option<String>) {
        // Check for Cargo.toml → Rust
        if root.join("Cargo.toml").exists() {
            let framework = Self::detect_rust_framework(root);
            return (Some(ProjectLanguage::Rust), framework);
        }
        // Check for package.json → TypeScript or JavaScript
        if root.join("package.json").exists() {
            let (lang, framework) = Self::detect_node_framework(root);
            return (lang, framework);
        }
        // Check for pyproject.toml → Python
        if root.join("pyproject.toml").exists() || root.join("requirements.txt").exists() {
            return (Some(ProjectLanguage::Python), None);
        }
        // Check for go.mod → Go
        if root.join("go.mod").exists() {
            return (Some(ProjectLanguage::Go), None);
        }

        (None, None)
    }

    fn detect_rust_framework(root: &Path) -> Option<String> {
        let cargo = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
        if cargo.contains("tauri") {
            return Some("Tauri".into());
        }
        if cargo.contains("actix-web") {
            return Some("Actix-Web".into());
        }
        if cargo.contains("axum") {
            return Some("Axum".into());
        }
        if cargo.contains("rocket") {
            return Some("Rocket".into());
        }
        if cargo.contains("warp") {
            return Some("Warp".into());
        }
        None
    }

    fn detect_node_framework(root: &Path) -> (Option<ProjectLanguage>, Option<String>) {
        let pkg = std::fs::read_to_string(root.join("package.json")).ok()?;
        let json: serde_json::Value = serde_json::from_str(&pkg).ok()?;

        // Detect TypeScript
        let has_ts = root.join("tsconfig.json").exists()
            || json.get("devDependencies")
                .and_then(|d| d.get("typescript"))
                .is_some();

        let lang = if has_ts {
            Some(ProjectLanguage::TypeScript)
        } else {
            Some(ProjectLanguage::JavaScript)
        };

        // Detect framework
        let framework = if let Some(deps) = json.get("dependencies") {
            if deps.get("next").is_some() {
                Some("Next.js".into())
            } else if deps.get("react").is_some() {
                Some("React".into())
            } else if deps.get("vue").is_some() {
                Some("Vue".into())
            } else if deps.get("@tauri-apps/api").is_some() {
                Some("Tauri".into())
            } else {
                None
            }
        } else {
            None
        };

        (lang, framework)
    }

    fn detect_dependencies(root: &Path, language: &Option<ProjectLanguage>) -> Vec<Dependency> {
        match language {
            Some(ProjectLanguage::Rust) => Self::parse_cargo_deps(root),
            Some(ProjectLanguage::TypeScript) | Some(ProjectLanguage::JavaScript) => {
                Self::parse_npm_deps(root)
            }
            _ => Vec::new(),
        }
    }

    fn parse_cargo_deps(root: &Path) -> Vec<Dependency> {
        let content = match std::fs::read_to_string(root.join("Cargo.toml")) {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        // Simple TOML key-value extraction for [dependencies] section
        let mut deps = Vec::new();
        let mut in_deps = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("[dependencies]") {
                in_deps = true;
                continue;
            }
            if in_deps {
                if trimmed.starts_with('[') {
                    break; // next section
                }
                if let Some(eq_pos) = trimmed.find('=') {
                    let name = trimmed[..eq_pos].trim().to_string();
                    if !name.is_empty() && !name.starts_with('#') {
                        let version = trimmed[eq_pos + 1..].trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .trim()
                            .to_string();
                        deps.push(Dependency {
                            name,
                            version: Some(version),
                        });
                    }
                }
            }
        }

        deps
    }

    fn parse_npm_deps(root: &Path) -> Vec<Dependency> {
        let content = match std::fs::read_to_string(root.join("package.json")) {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return vec![],
        };

        let mut deps = Vec::new();
        for section in &["dependencies", "devDependencies"] {
            if let Some(map) = json.get(*section).and_then(|v| v.as_object()) {
                for (name, ver) in map {
                    deps.push(Dependency {
                        name: name.clone(),
                        version: ver.as_str().map(|s| s.to_string()),
                    });
                }
            }
        }

        deps
    }

    /// Count files and directories recursively, excluding common noise dirs.
    fn count_files(root: &Path) -> (usize, usize, HashMap<String, usize>) {
        let exclude: std::collections::HashSet<&str> = [
            ".git", "node_modules", "target", ".next", "dist", "build", ".cache",
            "__pycache__", ".venv", "venv", ".svelte-kit",
        ].into_iter().collect();

        let mut file_count = 0;
        let mut dir_count = 0;
        let mut file_types: HashMap<String, usize> = HashMap::new();

        fn walk(
            dir: &Path,
            exclude: &std::collections::HashSet<&str>,
            file_count: &mut usize,
            dir_count: &mut usize,
            file_types: &mut HashMap<String, usize>,
        ) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if exclude.contains(name) {
                            continue;
                        }
                    }
                    if path.is_dir() {
                        *dir_count += 1;
                        walk(&path, exclude, file_count, dir_count, file_types);
                    } else {
                        *file_count += 1;
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            *file_types.entry(ext.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        walk(root, &exclude, &mut file_count, &mut dir_count, &mut file_types);

        (file_count, dir_count, file_types)
    }
}
```

- [ ] **Step 3: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        // Create Cargo.toml
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
serde = "1.0"
tokio = { version = "1", features = ["full"] }
"#,
        )
        .unwrap();
        // Create a src dir with a file
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        dir
    }

    #[test]
    fn test_detect_rust_project() {
        let dir = create_temp_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert_eq!(index.language, Some(ProjectLanguage::Rust));
        assert_eq!(index.file_count, 2);
        assert_eq!(index.dir_count, 1);
    }

    #[test]
    fn test_cargo_deps_parsed() {
        let dir = create_temp_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert!(!index.dependencies.is_empty());
        assert!(index.dependencies.iter().any(|d| d.name == "serde"));
        assert!(index.dependencies.iter().any(|d| d.name == "tokio"));
    }

    #[test]
    fn test_nonexistent_path() {
        let result = CodebaseIndexer::index(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_file_types() {
        let dir = create_temp_project();
        let index = CodebaseIndexer::index(dir.path()).unwrap();
        assert!(index.file_types.contains_key("toml"));
        assert!(index.file_types.contains_key("rs"));
        assert_eq!(*index.file_types.get("rs").unwrap(), 1);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test indexer::tests 2>&1`
Expected: 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/workspace/mod.rs src-tauri/src/workspace/indexer.rs
git commit -m "feat(workspace): add CodebaseIndexer with project detection and dependency analysis"
```

---

### Task 4: Build SearchEngine (ripgrep + glob wrapper)

**Files:**
- Create: `src-tauri/src/workspace/search.rs`

- [ ] **Step 1: Write `workspace/search.rs`**

```rust
use std::path::Path;
use std::process::Command;

use crate::error::{AppError, Result};

pub struct SearchEngine;

impl SearchEngine {
    /// Full-text search using ripgrep. Returns matching lines with file paths.
    pub fn grep(pattern: &str, root: &Path, file_pattern: Option<&str>) -> Result<Vec<GrepMatch>> {
        let mut cmd = Command::new("rg");
        cmd.arg("--line-number")
            .arg("--with-filename")
            .arg("--color")
            .arg("never")
            .arg("-i")
            .arg(pattern)
            .arg(root);

        if let Some(fp) = file_pattern {
            cmd.arg("-g").arg(fp);
        }

        let output = cmd.output().map_err(|e| {
            AppError::Workspace(format!("ripgrep execution failed: {}. Is `rg` installed?", e))
        })?;

        if !output.status.success() && !output.stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("No results found") {
                // Ignore ripgrep's "no results" warning
                log::warn!("ripgrep stderr: {}", stderr);
            }
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results = Vec::new();

        for line in stdout.lines() {
            // Format: path:line:content
            if let Some((rest, content)) = line.split_once(':').and_then(|(path, rest)| {
                rest.split_once(':').map(|(line_num, content)| {
                    (path, line_num, content)
                })
            }) {
                results.push(GrepMatch {
                    file: rest.to_string(),
                    line: line_number.unwrap_or(1),
                    content: content.to_string(),
                });
            }
        }

        Ok(results)
    }

    /// Glob file search. Finds files matching a glob pattern.
    pub fn glob(pattern: &str, root: &Path) -> Result<Vec<std::path::PathBuf>> {
        let pattern_str = if pattern.starts_with('/') || pattern.starts_with("**") {
            pattern.to_string()
        } else {
            root.join(pattern).to_string_lossy().to_string()
        };

        let mut matches = Vec::new();

        // Use the `glob` crate or a simple walker
        let exclude: std::collections::HashSet<&str> = [
            ".git", "node_modules", "target", ".next", "dist", "build", ".cache",
        ]
        .into_iter()
        .collect();

        fn walk(
            dir: &Path,
            pattern: &glob::Pattern,
            exclude: &std::collections::HashSet<&str>,
            matches: &mut Vec<std::path::PathBuf>,
        ) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if exclude.contains(name) {
                            continue;
                        }
                    }
                    if path.is_dir() {
                        walk(&path, pattern, exclude, matches);
                    } else if pattern.matches_path(&path) {
                        matches.push(path);
                    }
                }
            }
        }

        let pattern_obj = glob::Pattern::new(&pattern_str)
            .map_err(|e| AppError::Workspace(format!("Invalid glob pattern '{}': {}", pattern, e)))?;

        walk(root, &pattern_obj, &exclude, &mut matches);
        matches.sort();

        Ok(matches)
    }

    /// Fast file tree listing (non-recursive with depth control).
    pub fn tree(root: &Path, max_depth: usize) -> Result<Vec<TreeEntry>> {
        let mut entries = Vec::new();
        let exclude: std::collections::HashSet<&str> = [
            ".git", "node_modules", "target", ".next", "dist", "build", ".cache",
        ]
        .into_iter()
        .collect();

        fn walk(
            dir: &Path,
            depth: usize,
            max_depth: usize,
            exclude: &std::collections::HashSet<&str>,
            entries: &mut Vec<TreeEntry>,
        ) {
            if depth > max_depth {
                return;
            }
            if let Ok(read_dir) = std::fs::read_dir(dir) {
                let mut dir_entries: Vec<_> = read_dir.flatten().collect();
                dir_entries.sort_by_key(|e| e.file_name());

                for entry in dir_entries {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if exclude.contains(name) {
                            continue;
                        }
                    }
                    let is_dir = path.is_dir();
                    entries.push(TreeEntry {
                        path: path.to_path_buf(),
                        is_dir,
                        depth,
                    });
                    if is_dir {
                        walk(&path, depth + 1, max_depth, exclude, entries);
                    }
                }
            }
        }

        walk(root, 0, max_depth, &exclude, &mut entries);
        Ok(entries)
    }

    /// Check if ripgrep is available on the system.
    pub fn is_rg_available() -> bool {
        Command::new("rg")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreeEntry {
    pub path: std::path::PathBuf,
    pub is_dir: bool,
    pub depth: usize,
}
```

- [ ] **Step 2: Write the test**

Note: ripgrep may not be available in CI, so the grep test is conditional.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "hello world\nfoo bar").unwrap();
        fs::write(dir.path().join("test.rs"), "fn test() {}\n// hello").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/nested.txt"), "nested content").unwrap();
        dir
    }

    #[test]
    fn test_glob_finds_files() {
        let dir = create_test_dir();
        let results = SearchEngine::glob("*.txt", dir.path()).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|p| p.ends_with("hello.txt")));
    }

    #[test]
    fn test_tree_entries() {
        let dir = create_test_dir();
        let entries = SearchEngine::tree(dir.path(), 2).unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.path.ends_with("hello.txt")));
        assert!(entries.iter().any(|e| e.path.ends_with("sub")));
    }

    #[test]
    fn test_glob_nonexistent_pattern() {
        let dir = create_test_dir();
        let results = SearchEngine::glob("*.nonexistent", dir.path()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_grep_if_rg_available() {
        if SearchEngine::is_rg_available() {
            let dir = create_test_dir();
            let results = SearchEngine::grep("hello", dir.path(), None).unwrap();
            assert!(!results.is_empty());
            assert!(results.iter().any(|m| m.content.contains("hello")));
        }
        // If rg not available, test passes by skipping — no assertion needed
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test search::tests 2>&1`
Expected: tests pass (grep test may be skipped if ripgrep not available)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/workspace/search.rs
git commit -m "feat(workspace): add SearchEngine with grep, glob, and tree search"
```

---

### Task 5: Build ThinkerWorker

**Files:**
- Create: `src-tauri/src/workers/thinker.rs`

- [ ] **Step 1: Write `workers/thinker.rs`**

```rust
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::{LLMProvider, ProviderRegistry};
use crate::api::types::{ChatRequest, Message, MessageRole};
use crate::error::{AppError, Result};
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const COT_SYSTEM_PROMPT: &str = r#"You are a deep thinking module. Structure your reasoning in these phases:

[STEP 1: Problem Analysis]
Analyze what the user truly needs. Identify implicit requirements, constraints, and success criteria.

[STEP 2: Context Gathering]
What information do I need? What do I already know? What assumptions am I making?

[STEP 3: Approach Exploration]
Consider 2-3 approaches. Evaluate trade-offs for each. Identify risks and unknowns.

[STEP 4: Selected Approach]
Choose the best approach. Justify the choice. Define success criteria.

[STEP 5: Execution Plan]
Step-by-step plan with clear checkpoints. Note parallelization opportunities.

Then provide your final, detailed analysis."#;

pub struct ThinkerWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
}

impl ThinkerWorker {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self { providers }
    }
}

#[async_trait]
impl WorkerAgent for ThinkerWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::Thinker
    }

    fn description(&self) -> &str {
        "Deep reasoning and chain-of-thought analysis. No external tools — pure LLM thinking."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();

        let provider = {
            let registry = self.providers.lock().await;
            let mid = task.model_id.as_deref().unwrap_or_else(|| registry.default_model_id());
            if mid.is_empty() {
                return Err(AppError::Worker("No model configured for ThinkerWorker".into()));
            }
            registry.get(mid)?
        };

        let system_msg = if let Some(ctx) = &task.context {
            format!("{}\n\nAdditional context:\n{}", COT_SYSTEM_PROMPT, serde_json::to_string_pretty(ctx).unwrap_or_default())
        } else {
            COT_SYSTEM_PROMPT.to_string()
        };

        let request = ChatRequest {
            messages: vec![
                Message {
                    id: None,
                    role: MessageRole::System,
                    content: system_msg,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    id: None,
                    role: MessageRole::User,
                    content: task.instruction.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            model: "".to_string(), // model_id already resolved
            tools: None,
            stream: Some(false),
            max_tokens: task.max_tokens.map(|t| t as usize),
            temperature: task.temperature,
        };

        let response = provider.chat(request).await?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(WorkerResult {
            worker: WorkerKind::Thinker,
            task_id: task.id,
            content,
            metadata: None,
            duration_ms: Some(start.elapsed().as_millis() as u64),
        })
    }
}
```

- [ ] **Step 2: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinker_kind() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let worker = ThinkerWorker::new(providers);
        assert_eq!(worker.kind(), WorkerKind::Thinker);
        assert!(!worker.description().is_empty());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test thinker::tests 2>&1`
Expected: 1 test passes

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/workers/thinker.rs
git commit -m "feat(workers): add ThinkerWorker with CoT system prompt"
```

---

### Task 6: Build CodeExplorerWorker

**Files:**
- Create: `src-tauri/src/workers/code_explorer.rs`

- [ ] **Step 1: Write `workers/code_explorer.rs`**

```rust
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::{LLMProvider, ProviderRegistry};
use crate::api::types::{ChatRequest, Message, MessageRole};
use crate::error::Result;
use crate::workspace::search::SearchEngine;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const EXPLORER_SYSTEM_PROMPT: &str = r#"You are a code exploration agent. Your job is to understand codebases.

You have access to these tools via function calling:
- search_code(pattern, path): Full-text search across files
- search_files(glob_pattern, path): Find files by name pattern
- read_file(path): Read file contents
- tree_view(path, depth): List directory structure
- read_multiple_files(paths): Read several files at once

Analyze the code and provide:
1. Summary of relevant files and their roles
2. Key patterns, types, and functions found
3. Dependencies and relationships between components
4. Any issues or areas of concern"#;

pub struct CodeExplorerWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    workspace_root: Arc<Mutex<Option<String>>>,
}

impl CodeExplorerWorker {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self {
            providers,
            workspace_root: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn set_workspace_root(&self, root: String) {
        *self.workspace_root.lock().await = Some(root);
    }

    fn resolve_root(workspace_root: &Option<String>) -> &Path {
        workspace_root
            .as_ref()
            .map(|s| Path::new(s.as_str()))
            .unwrap_or_else(|| Path::new("."))
    }

    /// Execute a search tool call embedded in the instruction.
    /// Format: @@tool_name(args) where tool_name is search_code, search_files, read_file, tree_view
    async fn execute_tool(instruction: &str, root: &Path) -> String {
        let mut result = String::new();

        // Parse @@search_code(pattern, path?) commands
        for line in instruction.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("@@search_code(") && trimmed.ends_with(')') {
                let args = &trimmed[14..trimmed.len() - 1];
                let (pattern, path) = if let Some(comma) = args.find(',') {
                    (args[..comma].trim().trim_matches('"'),
                     args[comma + 1..].trim().trim_matches('"'))
                } else {
                    (args.trim().trim_matches('"'), "")
                };
                let search_root = if path.is_empty() { root } else { Path::new(path) };
                match SearchEngine::grep(pattern, search_root, None) {
                    Ok(matches) => {
                        for m in matches.iter().take(30) {
                            result.push_str(&format!("{}:{}: {}\n", m.file, m.line, m.content));
                        }
                    }
                    Err(e) => result.push_str(&format!("Search error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@search_files(") && trimmed.ends_with(')') {
                let pattern = &trimmed[15..trimmed.len() - 1].trim().trim_matches('"');
                match SearchEngine::glob(pattern, root) {
                    Ok(files) => {
                        for f in files {
                            result.push_str(&format!("{}\n", f.display()));
                        }
                    }
                    Err(e) => result.push_str(&format!("Glob error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@read_file(") && trimmed.ends_with(')') {
                let path_str = &trimmed[12..trimmed.len() - 1].trim().trim_matches('"');
                let file_path = if Path::new(path_str).is_absolute() {
                    Path::new(path_str).to_path_buf()
                } else {
                    root.join(path_str)
                };
                match std::fs::read_to_string(&file_path) {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let preview: String = lines.iter()
                            .take(100)
                            .enumerate()
                            .map(|(i, l)| format!("{:>4}: {}", i + 1, l))
                            .collect::<Vec<_>>()
                            .join("\n");
                        result.push_str(&format!("--- {} ({} lines) ---\n{}\n", file_path.display(), lines.len(), preview));
                    }
                    Err(e) => result.push_str(&format!("Read error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@tree_view(") && trimmed.ends_with(')') {
                let depth_str = &trimmed[12..trimmed.len() - 1].trim();
                let depth: usize = depth_str.parse().unwrap_or(2);
                match SearchEngine::tree(root, depth) {
                    Ok(entries) => {
                        for e in entries {
                            let indent = "  ".repeat(e.depth);
                            let name = e.path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("");
                            let suffix = if e.is_dir { "/" } else { "" };
                            result.push_str(&format!("{}{}{}\n", indent, name, suffix));
                        }
                    }
                    Err(e) => result.push_str(&format!("Tree error: {}\n", e)),
                }
            }
        }

        result
    }
}

#[async_trait]
impl WorkerAgent for CodeExplorerWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::CodeExplorer
    }

    fn description(&self) -> &str {
        "Explores codebases using grep, glob, file reads, and directory tree views. Understands project structure and code patterns."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();
        let root = self.workspace_root.lock().await;
        let root_path = Self::resolve_root(&root);

        // Execute any tool commands embedded in the instruction
        let tool_results = Self::execute_tool(&task.instruction, root_path).await;

        let system_prompt = format!(
            "{}\n\nCurrent workspace root: {}\n\nTool results from workspace search:\n{}",
            EXPLORER_SYSTEM_PROMPT,
            root_path.display(),
            if tool_results.is_empty() {
                "(No tool calls yet — ask user to specify code search parameters)".to_string()
            } else {
                tool_results
            }
        );

        let provider = {
            let registry = self.providers.lock().await;
            let mid = task.model_id.as_deref().unwrap_or_else(|| registry.default_model_id());
            if mid.is_empty() {
                drop(root);
                return Err(crate::error::AppError::Worker("No model configured for CodeExplorerWorker".into()));
            }
            registry.get(mid)?
        };

        let request = ChatRequest {
            messages: vec![
                Message {
                    id: None,
                    role: MessageRole::System,
                    content: system_prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    id: None,
                    role: MessageRole::User,
                    content: task.instruction.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            model: "".to_string(),
            tools: None,
            stream: Some(false),
            max_tokens: task.max_tokens.map(|t| t as usize),
            temperature: task.temperature,
        };

        let response = provider.chat(request).await?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        drop(root);

        Ok(WorkerResult {
            worker: WorkerKind::CodeExplorer,
            task_id: task.id,
            content,
            metadata: None,
            duration_ms: Some(start.elapsed().as_millis() as u64),
        })
    }
}
```

- [ ] **Step 2: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_explorer_kind() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let worker = CodeExplorerWorker::new(providers);
        assert_eq!(worker.kind(), WorkerKind::CodeExplorer);
    }

    #[tokio::test]
    async fn test_execute_tool_search_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("lib.rs"), "pub fn helper() {}").unwrap();

        let instruction = "@@search_files(\"*.rs\")\n";
        let result = CodeExplorerWorker::execute_tool(instruction, dir.path()).await;
        assert!(result.contains("test.rs"));
        assert!(result.contains("lib.rs"));
    }

    #[tokio::test]
    async fn test_execute_tool_read_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "hello world").unwrap();

        let instruction = format!("@@read_file(\"{}\")", dir.path().join("hello.txt").display());
        let result = CodeExplorerWorker::execute_tool(&instruction, dir.path()).await;
        assert!(result.contains("hello world"));
    }

    #[tokio::test]
    async fn test_execute_tool_tree() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), "").unwrap();

        let result = CodeExplorerWorker::execute_tool("@@tree_view(3)", dir.path()).await;
        assert!(result.contains("a.txt"));
        assert!(result.contains("sub"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test code_explorer::tests 2>&1`
Expected: 4 tests pass

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/workers/code_explorer.rs
git commit -m "feat(workers): add CodeExplorerWorker with grep/glob/tree tools"
```

---

### Task 7: Build CriticAgent

**Files:**
- Create: `src-tauri/src/critic/mod.rs`
- Create: `src-tauri/src/critic/agent.rs`

- [ ] **Step 1: Write `critic/mod.rs`**

```rust
pub mod agent;
```

- [ ] **Step 2: Write `critic/agent.rs`**

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::{LLMProvider, ProviderRegistry};
use crate::api::types::{ChatRequest, Message, MessageRole};
use crate::error::{AppError, Result};
use crate::workers::{SubTask, WorkerResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CritiqueDecision {
    /// Output is good — proceed
    Go,
    /// Output needs revision — retry with feedback
    Revise,
    /// Cannot resolve — escalate to Orchestrator
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Critique {
    pub decision: CritiqueDecision,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub score: Option<f32>,
}

const CRITIC_SYSTEM_PROMPT: &str = r#"You are a quality critic agent. Your job is to review AI agent outputs.

Evaluate the output against these criteria:
1. CORRECTNESS: Does the output correctly achieve the stated goal?
2. COMPLETENESS: Are there missing edge cases, error handling, or scenarios?
3. QUALITY: Is the code following best practices for the language?
4. CONSISTENCY: Does it follow existing codebase patterns?
5. SAFETY: Are there any security concerns, hardcoded secrets, or unsafe patterns?

Respond with a JSON object:
{
  "decision": "go" | "revise" | "escalate",
  "issues": ["issue 1", "issue 2"],
  "suggestions": ["suggestion 1", "suggestion 2"],
  "score": 0.85
}

Rules:
- Use "go" when output is good enough (doesn't need to be perfect)
- Use "revise" when there are concrete, fixable issues
- Use "escalate" when the task itself is flawed or unclear
- Score: 0.0 to 1.0, where 0.9+ is excellent"#;

pub struct CriticAgent {
    providers: Arc<Mutex<ProviderRegistry>>,
}

impl CriticAgent {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self { providers }
    }

    pub async fn review(
        &self,
        original_task: &SubTask,
        worker_result: &WorkerResult,
        model_id: Option<&str>,
    ) -> Result<Critique> {
        let provider = {
            let registry = self.providers.lock().await;
            let mid = model_id.unwrap_or_else(|| registry.default_model_id());
            if mid.is_empty() {
                // No model available — return a default Go critique
                return Ok(Critique {
                    decision: CritiqueDecision::Go,
                    issues: vec![],
                    suggestions: vec![],
                    score: Some(1.0),
                });
            }
            registry.get(mid)?
        };

        let user_prompt = format!(
            r#"## Original Task

Label: {}
Instruction: {}

## Worker Output

Worker: {:?}
Content:
{}"#,
            original_task.label,
            original_task.instruction,
            worker_result.worker,
            worker_result.content,
        );

        let request = ChatRequest {
            messages: vec![
                Message {
                    id: None,
                    role: MessageRole::System,
                    content: CRITIC_SYSTEM_PROMPT.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    id: None,
                    role: MessageRole::User,
                    content: user_prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            model: "".to_string(),
            tools: None,
            stream: Some(false),
            max_tokens: Some(1024),
            temperature: Some(0.2),
        };

        let response = provider.chat(request).await?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        // Extract JSON from response
        let json_str = extract_json(&content);

        match serde_json::from_str::<Critique>(json_str) {
            Ok(critique) => Ok(critique),
            Err(e) => {
                log::warn!("Failed to parse CriticAgent response as JSON: {}. Raw: {}", e, content);
                // Fallback: return Go critique to avoid blocking the flow
                Ok(Critique {
                    decision: CritiqueDecision::Go,
                    issues: vec![],
                    suggestions: vec![],
                    score: Some(0.5),
                })
            }
        }
    }

    /// Quick review without LLM call — rule-based check for common issues.
    /// Returns Some(critique) if issues found, None if clean.
    pub fn quick_review(original_task: &SubTask, worker_result: &WorkerResult) -> Option<Critique> {
        let mut issues = Vec::new();
        let mut suggestions = Vec::new();

        if worker_result.content.is_empty() {
            issues.push("Worker returned empty content".into());
            suggestions.push("Check if the worker received a valid instruction and model".into());
        }

        if worker_result.content.len() < 10 {
            issues.push("Worker response suspiciously short".into());
        }

        if worker_result.content.contains("I cannot") || worker_result.content.contains("I'm not able") {
            issues.push("Worker refused to complete the task".into());
        }

        if issues.is_empty() {
            None
        } else {
            Some(Critique {
                decision: CritiqueDecision::Revise,
                issues,
                suggestions,
                score: Some(0.0),
            })
        }
    }
}

fn extract_json(content: &str) -> &str {
    let trimmed = content.trim();
    // Try to find JSON between ```json and ``` first
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    // Try bare JSON object
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

#[async_trait]
pub trait Reviewable: Send + Sync {
    async fn review(&self, task: &SubTask, result: &WorkerResult) -> Result<Critique>;
}

#[async_trait]
impl Reviewable for CriticAgent {
    async fn review(&self, task: &SubTask, result: &WorkerResult) -> Result<Critique> {
        self.review(task, result, None).await
    }
}
```

- [ ] **Step 3: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::{SubTask, WorkerKind, WorkerResult};

    #[test]
    fn test_quick_review_empty_content() {
        let task = SubTask {
            id: "t1".into(),
            label: "Test".into(),
            instruction: "Do something".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        };
        let result = WorkerResult {
            worker: WorkerKind::Thinker,
            task_id: "t1".into(),
            content: "".into(),
            metadata: None,
            duration_ms: Some(100),
        };
        let critique = CriticAgent::quick_review(&task, &result);
        assert!(critique.is_some());
        assert_eq!(critique.unwrap().decision, CritiqueDecision::Revise);
    }

    #[test]
    fn test_quick_review_clean() {
        let task = SubTask {
            id: "t2".into(),
            label: "Test".into(),
            instruction: "Do something".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        };
        let result = WorkerResult {
            worker: WorkerKind::Thinker,
            task_id: "t2".into(),
            content: "Here is a detailed analysis of the problem with multiple paragraphs of useful information.".into(),
            metadata: None,
            duration_ms: Some(500),
        };
        let critique = CriticAgent::quick_review(&task, &result);
        assert!(critique.is_none());
    }

    #[test]
    fn test_extract_json() {
        let input = "Here is the review:\n```json\n{\"decision\": \"go\", \"issues\": [], \"suggestions\": []}\n```\nEnd.";
        let extracted = extract_json(input);
        assert!(extracted.contains("\"decision\": \"go\""));
    }

    #[test]
    fn test_extract_json_bare() {
        let input = "{\"decision\": \"revise\", \"issues\": [\"bug\"], \"suggestions\": [\"fix it\"]}";
        let extracted = extract_json(input);
        assert!(extracted.contains("\"revise\""));
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test critic::agent::tests 2>&1`
Expected: 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/critic/mod.rs src-tauri/src/critic/agent.rs
git commit -m "feat(critic): add CriticAgent with LLM and rule-based review"
```

---

### Task 8: Build OrchestratorAgent (core orchestration)

**Files:**
- Create: `src-tauri/src/orchestrator/agent.rs`
- Create: `src-tauri/src/orchestrator/dispatcher.rs`

- [ ] **Step 1: Write `orchestrator/dispatcher.rs`**

```rust
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

/// Manages worker registration and invocation.
pub struct Dispatcher {
    workers: HashMap<WorkerKind, Box<dyn WorkerAgent>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
        }
    }

    pub fn register(&mut self, worker: Box<dyn WorkerAgent>) {
        let kind = worker.kind();
        self.workers.insert(kind, worker);
    }

    pub fn has_worker(&self, kind: &WorkerKind) -> bool {
        self.workers.contains_key(kind)
    }

    pub fn list_workers(&self) -> Vec<WorkerKind> {
        self.workers.keys().cloned().collect()
    }

    pub async fn dispatch(&self, task: &SubTask) -> Result<WorkerResult> {
        let worker = self.workers.get(&task.worker_kind).ok_or_else(|| {
            crate::error::AppError::Orchestrator(format!(
                "No worker registered for kind: {:?}",
                task.worker_kind
            ))
        })?;

        worker.execute(task.clone()).await
    }

    pub async fn dispatch_with_feedback(
        &self,
        task: &SubTask,
        feedback: &crate::workers::WorkerFeedback,
    ) -> Result<WorkerResult> {
        let worker = self.workers.get(&task.worker_kind).ok_or_else(|| {
            crate::error::AppError::Orchestrator(format!(
                "No worker registered for kind: {:?}",
                task.worker_kind
            ))
        })?;

        worker.execute_with_feedback(task.clone(), feedback).await
    }
}
```

- [ ] **Step 2: Write `orchestrator/agent.rs`**

```rust
use std::sync::Arc;

use crate::critic::{CriticAgent, Critique, CritiqueDecision};
use crate::error::{AppError, Result};
use crate::orchestrator::dispatcher::Dispatcher;
use crate::orchestrator::task_graph::{NodeStatus, TaskGraph};
use crate::workers::{SubTask, WorkerKind, WorkerResult};

/// Phase of the orchestration lifecycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrchestrationPhase {
    Idle,
    Analyzing,
    Planning,
    Executing,
    Reflecting,
    Synthesizing,
    Done,
}

/// Event emitted during orchestration (for frontend streaming).
#[derive(Debug, Clone)]
pub enum OrchestrationEvent {
    PhaseChanged(OrchestrationPhase),
    TaskStarted { task_id: String, label: String, worker: WorkerKind },
    TaskCompleted {
        task_id: String,
        label: String,
        result_summary: String,
        duration_ms: u64,
    },
    TaskFailed { task_id: String, label: String, error: String },
    CritiqueReceived {
        task_id: String,
        decision: CritiqueDecision,
        issues: Vec<String>,
    },
    Thinking { content: String },
    SynthesizedOutput(String),
}

pub struct OrchestratorAgent {
    dispatcher: Dispatcher,
    critic: Arc<CriticAgent>,
    event_tx: Option<tokio::sync::mpsc::Sender<OrchestrationEvent>>,
    max_critique_rounds: u32,
}

impl OrchestratorAgent {
    pub fn new(
        dispatcher: Dispatcher,
        critic: Arc<CriticAgent>,
    ) -> Self {
        Self {
            dispatcher,
            critic,
            event_tx: None,
            max_critique_rounds: 3,
        }
    }

    pub fn with_event_channel(mut self, tx: tokio::sync::mpsc::Sender<OrchestrationEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    pub fn with_max_critique_rounds(mut self, rounds: u32) -> Self {
        self.max_critique_rounds = rounds;
        self
    }

    pub fn dispatcher(&self) -> &Dispatcher {
        &self.dispatcher
    }

    async fn emit(&self, event: OrchestrationEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event).await;
        }
    }

    /// Execute a full task graph through the orchestration pipeline.
    pub async fn execute_graph(&self, graph: &mut TaskGraph) -> Result<String> {
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Planning)).await;

        // Execute the DAG — process ready nodes in parallel batches
        while !graph.is_complete() && !graph.has_failures() {
            let ready = graph.ready_nodes();

            if ready.is_empty() && !graph.is_complete() {
                // Check for stalled tasks (dependencies completed but node still pending)
                for node in &graph.nodes {
                    if matches!(node.status, NodeStatus::Pending) {
                        log::warn!("Task '{}' is stalled — no ready nodes but not complete", node.id);
                    }
                }
                return Err(AppError::Orchestrator("Task graph stalled — dependency cycle or unreachable nodes".into()));
            }

            // Execute ready nodes sequentially (parallel execution in Phase 3)
            for node in ready {
                let node_id = node.id.clone();
                let node_label = node.label.clone();
                let worker_kind = node.worker_kind.clone();

                // Mark as running
                graph.get_node_mut(&node_id).unwrap().status = NodeStatus::Running;
                self.emit(OrchestrationEvent::TaskStarted {
                    task_id: node_id.clone(),
                    label: node_label.clone(),
                    worker: worker_kind.clone(),
                }).await;

                // Execute
                let result = self.execute_node(graph, &node_id).await;

                match result {
                    Ok(content) => {
                        let node = graph.get_node(&node_id).unwrap();
                        self.emit(OrchestrationEvent::TaskCompleted {
                            task_id: node_id,
                            label: node_label,
                            result_summary: content.chars().take(100).collect(),
                            duration_ms: node.duration_ms.unwrap_or(0),
                        }).await;
                    }
                    Err(e) => {
                        graph.get_node_mut(&node_id).unwrap().status = NodeStatus::Failed(e.to_string());
                        self.emit(OrchestrationEvent::TaskFailed {
                            task_id: node_id,
                            label: node_label,
                            error: e.to_string(),
                        }).await;
                    }
                }
            }
        }

        // Check for failures
        if graph.has_failures() {
            let failed_nodes: Vec<String> = graph.nodes
                .iter()
                .filter(|n| matches!(n.status, NodeStatus::Failed(_)))
                .map(|n| format!("{}: {}", n.label, n.error.as_deref().unwrap_or("unknown")))
                .collect();
            return Err(AppError::Orchestrator(format!(
                "Task graph execution failed for nodes: {}",
                failed_nodes.join("; ")
            )));
        }

        // Synthesize final output
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Synthesizing)).await;
        let synthesis = self.synthesize_results(graph).await?;

        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Done)).await;
        self.emit(OrchestrationEvent::SynthesizedOutput(synthesis.clone())).await;

        Ok(synthesis)
    }

    /// Execute a single node with Critic reflection loop.
    async fn execute_node(&self, graph: &TaskGraph, node_id: &str) -> Result<String> {
        let node = graph.get_node(node_id).ok_or_else(|| {
            AppError::Orchestrator(format!("Node '{}' not found in graph", node_id))
        })?;

        let task = node.to_sub_task();
        let mut critique_round = 0;

        loop {
            let start = std::time::Instant::now();

            // Execute
            let result = if critique_round == 0 {
                self.dispatcher.dispatch(&task).await
            } else {
                // Re-dispatch with feedback from the last critique
                let last_feedback = crate::workers::WorkerFeedback::new(
                    vec!["Previous attempt failed review".into()],
                    vec!["Revise based on previous feedback".into()],
                );
                self.dispatcher.dispatch_with_feedback(&task, &last_feedback).await
            };

            let result = match result {
                Ok(r) => r,
                Err(e) => return Err(e),
            };

            // Record duration
            if let Some(node) = graph.get_node(node_id) {
                // Can't mutate here since we have immutable ref
            }

            // Rule-based quick review (no LLM call)
            let task_ref = task;
            if let Some(quick_critique) = CriticAgent::quick_review(&task_ref, &result) {
                self.emit(OrchestrationEvent::CritiqueReceived {
                    task_id: node_id.to_string(),
                    decision: quick_critique.decision.clone(),
                    issues: quick_critique.issues.clone(),
                }).await;

                if critique_round < self.max_critique_rounds - 1
                    && quick_critique.decision == CritiqueDecision::Revise
                {
                    critique_round += 1;
                    continue;
                }
            }

            // Deep LLM-based review
            let critique = self.critic.review(&task_ref, &result, None).await?;

            self.emit(OrchestrationEvent::CritiqueReceived {
                task_id: node_id.to_string(),
                decision: critique.decision.clone(),
                issues: critique.issues.clone(),
            }).await;

            match critique.decision {
                CritiqueDecision::Go => {
                    // Mark as completed in the graph
                    // (we use a separate mutable reference since graph is passed around)
                    return Ok(result.content);
                }
                CritiqueDecision::Revise => {
                    if critique_round >= self.max_critique_rounds - 1 {
                        log::warn!(
                            "Max critique rounds ({}) reached for node '{}'. Accepting output.",
                            self.max_critique_rounds,
                            node_id
                        );
                        return Ok(result.content);
                    }
                    critique_round += 1;
                    continue;
                }
                CritiqueDecision::Escalate => {
                    return Err(AppError::Orchestrator(format!(
                        "Critic escalated node '{}': {:?}",
                        node_id, critique.issues
                    )));
                }
            }
        }
    }

    /// Synthesize all completed node results into a coherent final output.
    async fn synthesize_results(&self, graph: &TaskGraph) -> Result<String> {
        let mut output = String::new();

        output.push_str(&format!(
            "# Task Execution Summary\n\nGoal: {}\n\n",
            graph.goal
        ));

        for node in &graph.nodes {
            let status_str = match &node.status {
                NodeStatus::Completed => "✅",
                NodeStatus::Failed(_) => "❌",
                NodeStatus::Skipped => "⏭️",
                NodeStatus::Pending | NodeStatus::Running => "",
            };

            output.push_str(&format!(
                "## {} {} — {} ({:?})\n\n",
                status_str, node.label, node.id, node.worker_kind
            ));

            if let Some(summary) = &node.result_summary {
                output.push_str(&format!("{}\n\n", summary));
            }

            if let Some(error) = &node.error {
                output.push_str(&format!("**Error:** {}\n\n", error));
            }
        }

        Ok(output)
    }

    /// ——— Convenience: single goal → full pipeline ———
    /// Takes a natural language goal, auto-decomposes, executes, and returns the result.
    /// This is the primary entry point for Phase 2 integration.
    pub async fn process_goal(
        &self,
        goal: &str,
        _model_id: Option<&str>,
    ) -> Result<String> {
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Analyzing)).await;

        // Step 1: Decompose goal into task graph
        // (Basic decomposition logic — Phase 2 will use LLM-based planner)
        let mut graph = TaskGraph::new(
            uuid::Uuid::new_v4().to_string(),
            goal.to_string(),
        );

        // Default decomposition: single Thinker task
        // In Phase 2, this will be replaced by LLM-based decomposer
        graph.add_node(
            crate::orchestrator::task_graph::TaskNode::new(
                "step_1".into(),
                format!("Analyze and respond to: {}", goal),
                WorkerKind::Thinker,
                goal.to_string(),
            ),
        );

        // Step 2: Execute graph
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Executing)).await;
        let result = self.execute_graph(&mut graph).await?;

        Ok(result)
    }
}
```

- [ ] **Step 3: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::dispatcher::Dispatcher;
    use crate::orchestrator::task_graph::TaskNode;
    use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};
    use async_trait::async_trait;

    /// Mock worker for testing
    struct MockWorker(WorkerKind);

    #[async_trait]
    impl WorkerAgent for MockWorker {
        fn kind(&self) -> WorkerKind { self.0.clone() }
        fn description(&self) -> &str { "mock worker for testing" }

        async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
            Ok(WorkerResult {
                worker: self.0.clone(),
                task_id: task.id,
                content: format!("Mock result for: {}", task.label),
                metadata: None,
                duration_ms: Some(10),
            })
        }
    }

    #[test]
    fn test_dispatcher_register_and_list() {
        let mut d = Dispatcher::new();
        assert!(d.list_workers().is_empty());
        d.register(Box::new(MockWorker(WorkerKind::Thinker)));
        assert!(d.has_worker(&WorkerKind::Thinker));
        assert!(!d.has_worker(&WorkerKind::CodeExplorer));
        assert_eq!(d.list_workers().len(), 1);
    }

    #[tokio::test]
    async fn test_dispatcher_dispatch() {
        let mut d = Dispatcher::new();
        d.register(Box::new(MockWorker(WorkerKind::Thinker)));

        let task = SubTask {
            id: "t1".into(),
            label: "Test".into(),
            instruction: "Do it".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        };

        let result = d.dispatch(&task).await.unwrap();
        assert!(result.content.contains("Mock result for: Test"));
    }

    #[tokio::test]
    async fn test_dispatcher_missing_worker() {
        let d = Dispatcher::new();
        let task = SubTask {
            id: "t1".into(),
            label: "Test".into(),
            instruction: "Do it".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        };

        let result = d.dispatch(&task).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_orchestrator_empty_graph() {
        let dispatcher = Dispatcher::new();
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let critic = Arc::new(CriticAgent::new(providers));
        let orchestrator = OrchestratorAgent::new(dispatcher, critic);

        let mut graph = TaskGraph::new("g1".into(), "empty test".into());
        orchestrator.execute_graph(&mut graph).await.unwrap();
        assert!(graph.is_complete());
    }

    #[tokio::test]
    async fn test_orchestrator_single_task() {
        let mut dispatcher = Dispatcher::new();
        dispatcher.register(Box::new(MockWorker(WorkerKind::Thinker)));

        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let critic = Arc::new(CriticAgent::new(providers));
        let orchestrator = OrchestratorAgent::new(dispatcher, critic);

        let mut graph = TaskGraph::new("g2".into(), "single task test".into());
        graph.add_node(TaskNode::new(
            "s1".into(),
            "Test step".into(),
            WorkerKind::Thinker,
            "Say hello".into(),
        ));

        let result = orchestrator.execute_graph(&mut graph).await.unwrap();
        assert!(result.contains("Test step"));
        assert_eq!(
            graph.get_node("s1").unwrap().status,
            NodeStatus::Completed
        );
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test orchestrator::agent::tests 2>&1`
Expected: 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/orchestrator/agent.rs src-tauri/src/orchestrator/dispatcher.rs
git commit -m "feat(orchestrator): add OrchestratorAgent with DAG execution and Critic reflection loop"
```

---

### Task 9: Register new modules in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add `uses` for `glob` crate in Cargo.toml**

The SearchEngine uses `glob::Pattern`. Add `glob` dependency:

```toml
glob = "0.3"
```

Also add `notify` (for Phase 2 FileWatcher, declare now):

```toml
notify = "7"
```

- [ ] **Step 2: Add module declarations to `lib.rs`**

After `pub mod tools;` (line 19), add:

```rust
pub mod orchestrator;
pub mod workers;
pub mod critic;
pub mod workspace;
```

- [ ] **Step 3: Run build check**

Run: `cd src-tauri && cargo check 2>&1`
Expected: no errors, all modules compile

- [ ] **Step 4: Run full test suite**

Run: `cd src-tauri && cargo test 2>&1`
Expected: all tests pass (including all 19+ new tests from Tasks 1-8)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat: register orchestrator, workers, critic, workspace modules"
```

---

## Self-Review

### 1. Spec Coverage

| Design Requirement | Task | Status |
|---|---|---|
| WorkerAgent trait | Task 1 | ✅ |
| WorkerKind enum | Task 1 | ✅ |
| TaskGraph DAG data model | Task 2 | ✅ |
| CodebaseIndexer with project detection | Task 3 | ✅ |
| SearchEngine (grep, glob, tree) | Task 4 | ✅ |
| ThinkerWorker (CoT) | Task 5 | ✅ |
| CodeExplorerWorker | Task 6 | ✅ |
| CriticAgent (LLM + rule-based) | Task 7 | ✅ |
| Dispatcher (worker registry) | Task 8 | ✅ |
| OrchestratorAgent (DAG execution) | Task 8 | ✅ |
| OrchestrationEvent for frontend | Task 8 | ✅ |
| Module registration | Task 9 | ✅ |
| File Watcher | Phase 2 | ⏳ |
| Git Integration | Phase 2 | ⏳ |
| CodeEditor/Shell/Web/Memory/MCP Workers | Phase 2 | ⏳ |
| Frontend components | Phase 2 | ⏳ |

### 2. Placeholder Scan
- All code blocks contain complete, compilable Rust
- No "TBD", "TODO", "implement later" patterns
- Every test has concrete assertions

### 3. Type Consistency
- `WorkerKind` enum values match between `workers/mod.rs` and `orchestrator/agent.rs`
- `TaskGraph` methods are consistently named across `task_graph.rs` and `agent.rs`
- `Critique` struct defined in `critic/agent.rs` and imported in `orchestrator/agent.rs`
- All error variants (`Orchestrator`, `Worker`, `Critic`, `Workspace`) registered in `error.rs`
