use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use crate::llm::provider::ProviderRegistry;
use crate::llm::util::extract_json;
use crate::agent::orchestrator::plan_error::ExecutionError;
use crate::agent::orchestrator::plan_types::*;
use crate::agent::tools::registry::ToolRegistry;

/// LLM Planner — 将用户自然语言目标转为 ExecutionPlan
pub struct LlmPlanner {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
}

impl LlmPlanner {
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
    ) -> Self {
        Self { providers, tools }
    }

    /// Generate an execution plan with the default policy
    /// (no tool whitelist, tools enabled, no conversation context).
    pub async fn generate_plan(
        &self,
        goal: &str,
        session_id: &str,
        model_id: Option<&str>,
    ) -> Result<ExecutionPlan, ExecutionError> {
        self.generate_plan_with_policy(goal, session_id, model_id, None, true, None)
            .await
    }

    /// Generate an execution plan constrained by the turn's tool policy.
    ///
    /// `allowed_tools` narrows the tool catalog to the whitelist; an empty slice
    /// means "no tools". `tools_enabled == false` disables tools entirely.
    /// `context` is an already-truncated recent conversation block prepended to
    /// the goal.
    pub async fn generate_plan_with_policy(
        &self,
        goal: &str,
        session_id: &str,
        model_id: Option<&str>,
        allowed_tools: Option<&[String]>,
        tools_enabled: bool,
        context: Option<&str>,
    ) -> Result<ExecutionPlan, ExecutionError> {
        // Collect the visible tool catalog (before taking the providers lock).
        let tool_lines = {
            let registry = self.tools.lock().await;
            let available: Vec<(String, String)> = registry
                .get_enabled()
                .iter()
                .map(|tool| {
                    let def = tool.definition();
                    (
                        def.name.clone(),
                        format!(
                            "- `{}`: {} — params: {}",
                            def.name, def.description, def.parameters
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
                .collect::<Vec<String>>()
        };

        // 获取模型
        let providers = self.providers.lock().await;
        let mid = match model_id {
            Some(m) => m.to_string(),
            None => {
                let default = providers.default_model_id();
                if default.is_empty() {
                    drop(providers);
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

        let planner_system = build_planner_prompt(&tool_lines, tools_enabled);
        let user_message = build_planner_user_message(goal, context);

        let content = provider.prompt(&planner_system, &user_message).await
            .map_err(|e| {
                ExecutionError::StepFailed {
                    step: 0,
                    message: format!("Planner LLM call failed: {}", e),
                }
            })?;

        // Extract JSON from response (may be wrapped in ```json ... ```)
        let json_str = extract_json(&content);

        let plan_value: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
            ExecutionError::Internal(format!(
                "Failed to parse planner response as JSON: {}. Raw: {}",
                e,
                &content[..content.len().min(200)]
            ))
        })?;

        let steps_array = plan_value
            .get("steps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ExecutionError::Internal("Planner response missing 'steps' array".to_string())
            })?;

        let mut steps = Vec::new();
        for (i, step_val) in steps_array.iter().enumerate() {
            let step_id = step_val
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("step_{}", i + 1))
                .to_string();
            let label = step_val
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("Step {}", i + 1))
                .to_string();
            let step_type = step_val
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("agent_task");

            let params = step_val.get("params");

            let execution = match step_type {
                "tool_call" => {
                    let tool_name = params
                        .and_then(|p| p.get("tool"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if tool_name.is_empty() {
                        // Empty tool name — fall back to AgentTask
                        log::warn!("Planner generated tool_call with empty tool name, falling back to agent_task");
                        ExecStep::AgentTask {
                            instruction: label.clone(),
                            model_id: None,
                            max_iterations: Some(10),
                            allowed_tools: None,
                            temperature: None,
                        }
                    } else {
                        ExecStep::ToolCall {
                            tool: tool_name.to_string(),
                            params: params
                                .and_then(|p| p.get("params"))
                                .and_then(|v| v.as_object())
                                .map(|m| {
                                    m.iter()
                                        .map(|(k, v)| (k.clone(), v.clone()))
                                        .collect()
                                })
                                .unwrap_or_default(),
                            retry: Some(RetryConfig {
                                max: 2,
                                delay_seconds: 1,
                            }),
                            timeout_seconds: None,
                        }
                    }
                },
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
            };

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
}

/// Narrow the full tool list to the tools the planner may see.
///
/// A disabled tool switch or an empty whitelist yields no tools at all.
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

/// Build the planner system prompt.
///
/// `tool_lines` are pre-rendered catalog entries (`- \`name\`: ...`). When tools
/// are disabled or the catalog is empty, the prompt explicitly forbids
/// `tool_call` steps instead of leaving the tool section blank.
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

/// Build the planner user message.
///
/// `context` is an already-truncated recent conversation block supplied by the
/// caller; this function only renders it ahead of the goal.
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
}
