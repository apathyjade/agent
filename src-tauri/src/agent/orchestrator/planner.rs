//! LLM 规划器：把自然语言目标拆解为可执行的 [`ExecutionPlan`]。
//!
//! 规划器只负责「目标 → 计划」；计划如何执行、如何落库由 `runtime` 负责。
//! 约束：规划器可见的工具目录必须与本次运行的策略一致，否则会生成
//! 运行时必然拒绝的 `tool_call` 步骤。

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use crate::core::util::truncate_chars;
use crate::llm::provider::ProviderRegistry;
use crate::llm::util::extract_json;
use crate::agent::orchestrator::plan_error::ExecutionError;
use crate::agent::orchestrator::plan_types::*;
use crate::agent::tools::registry::ToolRegistry;

/// 规划器失败时回显的原始响应长度上限（按字符计）。
const RAW_RESPONSE_PREVIEW_CHARS: usize = 200;

/// LLM Planner — 将用户自然语言目标转为 ExecutionPlan
pub struct LlmPlanner {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
}

impl LlmPlanner {
    /// 用共享的 provider 与工具注册表构造规划器。
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
    ) -> Self {
        Self { providers, tools }
    }

    /// 用默认策略生成执行计划（不限制工具、启用工具、无对话上下文）。
    pub async fn generate_plan(
        &self,
        goal: &str,
        session_id: &str,
        model_id: Option<&str>,
    ) -> Result<ExecutionPlan, ExecutionError> {
        self.generate_plan_with_policy(goal, session_id, model_id, None, true, None)
            .await
    }

    /// 在轮次工具策略约束下生成执行计划。
    ///
    /// `allowed_tools` 把工具目录收窄到白名单；空切片表示「不允许任何工具」。
    /// `tools_enabled == false` 时完全禁用工具。`context` 是调用方已截断的
    /// 近期对话片段，会拼接在目标之前。
    pub async fn generate_plan_with_policy(
        &self,
        goal: &str,
        session_id: &str,
        model_id: Option<&str>,
        allowed_tools: Option<&[String]>,
        tools_enabled: bool,
        context: Option<&str>,
    ) -> Result<ExecutionPlan, ExecutionError> {
        let tool_lines = self.collect_tool_catalog(allowed_tools, tools_enabled).await;

        let (mid, content) = self
            .call_planner(model_id, &tool_lines, tools_enabled, goal, context)
            .await?;

        let plan_value = parse_planner_json(&content)?;
        let steps_array = plan_value
            .get("steps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ExecutionError::Internal("Planner response missing 'steps' array".to_string())
            })?;
        let steps = parse_plan_steps(steps_array, goal);

        Ok(ExecutionPlan {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            source: PlanSource::Dynamic {
                goal: goal.to_string(),
                generated_by: mid.to_string(),
            },
            steps,
            status: PlanStatus::Pending,
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
        })
    }

    /// 收集规划器可见的工具目录（在获取 provider 锁之前完成）。
    async fn collect_tool_catalog(
        &self,
        allowed_tools: Option<&[String]>,
        tools_enabled: bool,
    ) -> Vec<String> {
        let registry = self.tools.lock().await;
        let available: Vec<(String, String)> = registry
            .catalog()
            .into_iter()
            .filter(|tool| tool.enabled)
            .map(|tool| {
                (
                    tool.name.clone(),
                    format!(
                        "- `{}`: {} — params: {}",
                        tool.name, tool.description, tool.parameters
                    ),
                )
            })
            .collect();

        let names: Vec<String> = available.iter().map(|(name, _)| name.clone()).collect();
        let visible = filter_allowed_tools(&names, allowed_tools, tools_enabled);
        visible
            .iter()
            .filter_map(|name| {
                available
                    .iter()
                    .find(|(candidate, _)| candidate == name)
                    .map(|(_, line)| line.clone())
            })
            .collect()
    }

    /// 解析模型并调用规划器，返回 `(模型 id, 原始响应)`。
    ///
    /// 拿到 provider 后立即释放注册表锁，避免 LLM 调用期间阻塞其他使用者。
    async fn call_planner(
        &self,
        model_id: Option<&str>,
        tool_lines: &[String],
        tools_enabled: bool,
        goal: &str,
        context: Option<&str>,
    ) -> Result<(String, String), ExecutionError> {
        let (mid, provider) = {
            let providers = self.providers.lock().await;
            let mid = match model_id {
                Some(m) => m.to_string(),
                None => {
                    let default = providers.default_model_id();
                    if default.is_empty() {
                        return Err(ExecutionError::Internal(
                            "No model ID configured for planner".to_string(),
                        ));
                    }
                    default.to_string()
                }
            };
            let provider = providers.get(&mid).map_err(|e| {
                ExecutionError::Internal(format!("Failed to get planner provider: {}", e))
            })?;
            (mid, provider)
        };

        let planner_system = build_planner_prompt(tool_lines, tools_enabled);
        let user_message = build_planner_user_message(goal, context);

        let content = provider
            .prompt(&planner_system, &user_message)
            .await
            .map_err(|e| ExecutionError::StepFailed {
                step: 0,
                message: format!("Planner LLM call failed: {}", e),
            })?;

        Ok((mid, content))
    }
}

/// 从规划器响应中提取并解析 JSON 对象。
///
/// 响应可能被 ```json 代码块包裹，因此先走 [`extract_json`]；
/// 解析失败时回显响应前 200 个字符，便于定位模型输出问题。
fn parse_planner_json(content: &str) -> Result<serde_json::Value, ExecutionError> {
    let json_str = extract_json(content);
    serde_json::from_str(&json_str).map_err(|e| {
        ExecutionError::Internal(format!(
            "Failed to parse planner response as JSON: {}. Raw: {}",
            e,
            truncate_chars(content, RAW_RESPONSE_PREVIEW_CHARS)
        ))
    })
}

/// 把 JSON 中的 `steps` 数组解析为 `PlanStep` 列表。
fn parse_plan_steps(steps_array: &[serde_json::Value], goal: &str) -> Vec<PlanStep> {
    let mut steps = Vec::with_capacity(steps_array.len());

    for (i, step_val) in steps_array.iter().enumerate() {
        let step_id = step_val
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("step_{}", i + 1));
        let label = step_val
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Step {}", i + 1));
        let step_type = step_val
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("agent_task");
        let params = step_val.get("params");

        let execution = parse_step_execution(step_type, params, &label, goal);

        steps.push(PlanStep {
            id: step_id,
            label,
            execution,
            status: StepStatus::Pending,
            result: None,
            error: None,
            started_at: None,
            duration_ms: None,
        });
    }

    steps
}

/// 按 `type` 与 `params` 解析单个步骤的执行体。
///
/// 未知类型统一回落到 `AgentTask`，保证计划始终可执行。
fn parse_step_execution(
    step_type: &str,
    params: Option<&serde_json::Value>,
    label: &str,
    goal: &str,
) -> ExecStep {
    match step_type {
        "tool_call" => {
            let tool_name = params
                .and_then(|p| p.get("tool"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if tool_name.is_empty() {
                // 工具名为空时回落到 AgentTask，避免生成无法执行的步骤。
                log::warn!("Planner generated tool_call with empty tool name, falling back to agent_task");
                agent_task_step(label, Some(10))
            } else {
                ExecStep::ToolCall {
                    tool: tool_name.to_string(),
                    params: params
                        .and_then(|p| p.get("params"))
                        .and_then(|v| v.as_object())
                        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default(),
                    retry: Some(RetryConfig {
                        max: 2,
                        delay_seconds: 1,
                    }),
                    timeout_seconds: None,
                }
            }
        }
        "agent_task" => ExecStep::AgentTask {
            instruction: params
                .and_then(|p| p.get("instruction"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            model_id: None,
            max_iterations: Some(15),
            allowed_tools: None,
            temperature: None,
        },
        "llm_call" => ExecStep::LlmCall {
            prompt: params
                .and_then(|p| p.get("prompt"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            system_prompt: params
                .and_then(|p| p.get("system_prompt"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            model_id: None,
            temperature: Some(0.3),
            max_tokens: Some(2048),
        },
        "condition" => ExecStep::Condition {
            expression: params
                .and_then(|p| p.get("expression"))
                .and_then(|v| v.as_str())
                .unwrap_or("true")
                .to_string(),
            on_true: BranchTarget::Continue,
            on_false: BranchTarget::End,
        },
        _ => ExecStep::AgentTask {
            instruction: format!("{}: {}", label, goal),
            model_id: None,
            max_iterations: Some(10),
            allowed_tools: None,
            temperature: None,
        },
    }
}

/// 构造一个使用默认参数的 agent 任务步骤。
fn agent_task_step(instruction: &str, max_iterations: Option<usize>) -> ExecStep {
    ExecStep::AgentTask {
        instruction: instruction.to_string(),
        model_id: None,
        max_iterations,
        allowed_tools: None,
        temperature: None,
    }
}

/// 把完整工具列表收窄到规划器可见的工具。
///
/// 工具被禁用或白名单为空时返回空列表。
fn filter_allowed_tools(
    all: &[String],
    allowed_tools: Option<&[String]>,
    tools_enabled: bool,
) -> Vec<String> {
    if !tools_enabled {
        return Vec::new();
    }

    match allowed_tools {
        Some(allowed) => {
            let allowed: HashSet<&str> = allowed.iter().map(String::as_str).collect();
            all.iter()
                .filter(|name| allowed.contains(name.as_str()))
                .cloned()
                .collect()
        }
        None => all.to_vec(),
    }
}

/// 构造规划器的系统提示词。
///
/// `tool_lines` 是预渲染的工具目录条目（`- \`name\`: ...`）。工具被禁用或
/// 目录为空时，提示词会明确禁止生成 `tool_call` 步骤，而不是留空。
fn build_planner_prompt(tool_lines: &[String], tools_enabled: bool) -> String {
    let has_tools = tools_enabled && !tool_lines.is_empty();

    let catalog = if has_tools {
        format!("Available tools:\n{}", tool_lines.join("\n"))
    } else {
        "No tools are available for this plan. You MUST NOT produce any `tool_call` steps, \
         and `agent_task` steps must not call tools. Use only `llm_call`, `condition`, \
         or purely textual `agent_task` steps."
            .to_string()
    };

    let tool_guidelines = if has_tools {
        "2. Use tool_call steps for single tool operations (read file, search, etc.)\n\
         3. Use agent_task steps for multi-step reasoning that requires LLM + tools\n"
    } else {
        "2. Never use tool_call steps — no tools are available this turn\n\
         3. Use agent_task steps only for purely textual reasoning; they have no tools either\n"
    };

    format!(
        r#"You are a planning expert. Your job is to decompose a user's goal into a structured execution plan.

{}

## Output Format

Respond with a JSON object ONLY, no other text:

```json
{{
  "steps": [
    {{
      "id": "step_1",
      "label": "Human-readable step description",
      "type": "tool_call | agent_task | llm_call | condition",
      "params": {{
        // type-specific parameters:
        // for tool_call: {{"tool": "tool_name", "params": {{...}}}}
        // for agent_task: {{"instruction": "what the agent should do"}}
        // for llm_call: {{"prompt": "prompt text"}}
        // for condition: {{"expression": "template expression"}}
      }}
    }}
  ]
}}
```

## Guidelines
1. Each step should be atomic and focused
{tool_guidelines}4. Use llm_call for pure text generation (no tools needed)
5. Use condition for branching logic
6. Keep the plan focused — minimum steps needed to achieve the goal
7. Label each step with a clear Chinese label describing what it does"#,
        catalog
    )
}

/// 构造规划器的用户消息。
///
/// `context` 是调用方已截断的近期对话片段；本函数只负责把它渲染在目标之前。
fn build_planner_user_message(goal: &str, context: Option<&str>) -> String {
    match context {
        Some(context) if !context.trim().is_empty() => format!(
            "## Recent conversation\n{}\n\nGoal: {}\n\nGenerate a plan to accomplish this goal.",
            context, goal
        ),
        _ => format!("Goal: {}\n\nGenerate a plan to accomplish this goal.", goal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归测试：规划器返回非 JSON 的中文内容时，错误路径上的响应预览
    /// 不能 panic（按字节切片会切在多字节字符中间）。
    #[test]
    fn parse_planner_json_reports_error_for_chinese_output_without_panicking() {
        let chinese = "规划器返回了一段无法解析的中文内容。".repeat(20);

        let err = parse_planner_json(&chinese).expect_err("中文内容不是合法 JSON");

        let message = err.to_string();
        assert!(
            message.contains("Failed to parse planner response as JSON"),
            "错误消息应说明 JSON 解析失败，实际为：{}",
            message
        );
    }

    /// 回归测试：响应预览按字符（而非字节）截断，中文不会被切成半个字符。
    #[test]
    fn parse_planner_json_previews_by_characters_not_bytes() {
        let chinese = "规划器".repeat(200); // 600 字节 / 400 字符
        let err = parse_planner_json(&chinese).expect_err("中文内容不是合法 JSON");

        let message = err.to_string();
        let preview = message
            .split("Raw: ")
            .nth(1)
            .expect("错误消息应包含响应预览");
        assert_eq!(
            preview.chars().count(),
            RAW_RESPONSE_PREVIEW_CHARS,
            "预览应恰好是 {} 个字符",
            RAW_RESPONSE_PREVIEW_CHARS
        );
    }

    #[test]
    fn test_extract_json_with_code_block() {
        let input = "Here is the plan:\n```json\n{\"steps\": []}\n```\nEnd.";
        assert_eq!(extract_json(input), "{\"steps\": []}");
    }

    #[test]
    fn test_extract_json_plain() {
        let input = "{\"steps\": []}";
        assert_eq!(extract_json(input), "{\"steps\": []}");
    }

    #[test]
    fn test_extract_json_code_block_no_lang() {
        let input = "```\n{\"steps\": []}\n```";
        assert_eq!(extract_json(input), "{\"steps\": []}");
    }

    fn catalog_line(name: &str) -> String {
        format!("- `{}`: test tool — params: {{}}", name)
    }

    #[test]
    fn test_filter_allowed_tools_excludes_non_whitelisted() {
        let all = vec![
            "file_system".to_string(),
            "web_search".to_string(),
            "calculator".to_string(),
        ];
        let allowed = vec!["file_system".to_string()];

        let result = filter_allowed_tools(&all, Some(&allowed), true);

        assert_eq!(result, vec!["file_system".to_string()]);
        assert!(!result.contains(&"web_search".to_string()));
        assert!(!result.contains(&"calculator".to_string()));
    }

    #[test]
    fn test_filter_allowed_tools_empty_whitelist_yields_nothing() {
        let all = vec!["file_system".to_string()];
        let allowed: Vec<String> = Vec::new();

        assert!(filter_allowed_tools(&all, Some(&allowed), true).is_empty());
    }

    #[test]
    fn test_filter_allowed_tools_disabled_yields_nothing() {
        let all = vec!["file_system".to_string()];

        assert!(filter_allowed_tools(&all, None, false).is_empty());
    }

    #[test]
    fn test_filter_allowed_tools_default_policy_keeps_all() {
        let all = vec!["file_system".to_string(), "web_search".to_string()];

        assert_eq!(filter_allowed_tools(&all, None, true), all);
    }

    #[test]
    fn test_prompt_without_tools_forbids_tool_calls() {
        let prompt = build_planner_prompt(&[], false);

        assert!(prompt.contains("No tools are available"));
        assert!(prompt.contains("MUST NOT produce any `tool_call` steps"));
        assert!(!prompt.contains("- `"));
    }

    #[test]
    fn test_prompt_with_empty_whitelist_forbids_tool_calls() {
        let prompt = build_planner_prompt(&[], true);

        assert!(prompt.contains("No tools are available"));
        assert!(prompt.contains("MUST NOT produce any `tool_call` steps"));
        assert!(!prompt.contains("- `"));
    }

    #[test]
    fn test_prompt_lists_only_given_tools() {
        let lines = vec![catalog_line("file_system")];

        let prompt = build_planner_prompt(&lines, true);

        assert!(prompt.contains("- `file_system`"));
        assert!(!prompt.contains("web_search"));
        assert!(!prompt.contains("No tools are available"));
    }

    #[test]
    fn test_context_precedes_goal() {
        let message = build_planner_user_message("do it", Some("user: earlier\nassistant: ok"));

        let context_pos = message.find("Recent conversation").unwrap();
        let goal_pos = message.find("Goal: do it").unwrap();
        assert!(context_pos < goal_pos);
    }

    #[test]
    fn test_no_context_keeps_goal_shape() {
        let message = build_planner_user_message("do it", None);

        assert!(message.starts_with("Goal: do it"));
        assert!(!message.contains("Recent conversation"));
    }

    #[test]
    fn test_blank_context_is_ignored() {
        let message = build_planner_user_message("do it", Some("   \n  "));

        assert!(!message.contains("Recent conversation"));
    }

    #[test]
    fn test_parse_plan_steps_parses_each_type() {
        let steps = serde_json::json!([
            {"id": "s1", "label": "L1", "type": "tool_call", "params": {"tool": "calculator", "params": {"expression": "1+1"}}},
            {"id": "s2", "label": "L2", "type": "llm_call", "params": {"prompt": "hi"}},
            {"id": "s3", "label": "L3", "type": "condition", "params": {"expression": "true"}},
            {"id": "s4", "label": "L4", "type": "unknown"}
        ]);
        let array = steps.as_array().expect("array");
        let parsed = parse_plan_steps(array, "goal");

        assert_eq!(parsed.len(), 4);
        assert!(matches!(parsed[0].execution, ExecStep::ToolCall { .. }));
        assert!(matches!(parsed[1].execution, ExecStep::LlmCall { .. }));
        assert!(matches!(parsed[2].execution, ExecStep::Condition { .. }));
        assert!(matches!(parsed[3].execution, ExecStep::AgentTask { .. }));
    }
}
