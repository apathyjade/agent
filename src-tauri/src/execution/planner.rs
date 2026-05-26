use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::api::types::{ChatRequest, Message as ApiMessage, MessageRole};
use crate::execution::error::ExecutionError;
use crate::execution::types::*;
use crate::tools::registry::ToolRegistry;

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

    /// 生成执行计划
    pub async fn generate_plan(
        &self,
        goal: &str,
        session_id: &str,
        model_id: Option<&str>,
    ) -> Result<ExecutionPlan, ExecutionError> {
        // 获取工具列表（在获取 providers 锁之前）
        let tools_list = {
            let registry = self.tools.lock().await;
            registry
                .get_enabled()
                .iter()
                .map(|t| {
                    format!(
                        "- `{}`: {} — params: {}",
                        t.name(),
                        t.description(),
                        t.parameters()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
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

        let planner_system = format!(
            r#"You are a planning expert. Your job is to decompose a user's goal into a structured execution plan.

Available tools:
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
2. Use tool_call steps for single tool operations (read file, search, etc.)
3. Use agent_task steps for multi-step reasoning that requires LLM + tools
4. Use llm_call for pure text generation (no tools needed)
5. Use condition for branching logic
6. Keep the plan focused — minimum steps needed to achieve the goal
7. Label each step with a clear Chinese label describing what it does"#,
            tools_list
        );

        let user_message = format!(
            "Goal: {}\n\nGenerate a plan to accomplish this goal.",
            goal
        );

        let messages = vec![
            ApiMessage {
                id: None,
                role: MessageRole::System,
                content: planner_system,
                tool_calls: None,
                tool_call_id: None,
            },
            ApiMessage {
                id: None,
                role: MessageRole::User,
                content: user_message,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let request = ChatRequest {
            messages,
            model: mid.to_string(),
            tools: None,
            stream: Some(false),
            max_tokens: Some(4096),
            temperature: Some(0.3),
        };

        let response = provider.chat(request).await.map_err(|e| {
            ExecutionError::StepFailed {
                step: 0,
                message: format!("Planner LLM call failed: {}", e),
            }
        })?;

        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

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

/// 从 LLM 响应中提取 JSON（去除 markdown 代码块标记）
fn extract_json(content: &str) -> &str {
    let trimmed = content.trim();
    // 处理 ```json ... ``` 包装
    if let Some(start) = trimmed.find("```json") {
        let after_start = &trimmed[start + 7..];
        if let Some(end) = after_start.find("```") {
            return after_start[..end].trim();
        }
        return after_start.trim();
    }
    // 处理 ``` ... ``` 包装（无语言标记）
    if let Some(start) = trimmed.find("```") {
        let after_start = &trimmed[start + 3..];
        if let Some(end) = after_start.find("```") {
            return after_start[..end].trim();
        }
        return after_start.trim();
    }
    trimmed
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
}
