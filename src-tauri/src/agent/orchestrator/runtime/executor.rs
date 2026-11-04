// Step-level execution for `ExecutionRuntime`.
//
// Plan orchestration (`execute`) and persistence helpers stay in `runtime.rs`;
// per-step dispatch (tool / llm / agent / pipeline / condition) lives here.

use super::*;

impl ExecutionRuntime {
    /// 执行单个 ExecStep
    pub(super) async fn execute_step(
        &self,
        step: &ExecStep,
        _step_id: &str,
        cancel_flag: Arc<AtomicBool>,
        event_tx: mpsc::Sender<PlanProgressEvent>,
    ) -> Result<Value, ExecutionError> {
        match step {
            ExecStep::ToolCall {
                tool,
                params,
                retry,
                timeout_seconds,
            } => {
                self.execute_tool(tool, params, retry, *timeout_seconds, cancel_flag)
                    .await
            }
            ExecStep::LlmCall {
                prompt,
                system_prompt,
                model_id,
                temperature,
                max_tokens,
            } => {
                self.execute_llm(prompt, system_prompt, model_id, *temperature, *max_tokens)
                    .await
            }
            ExecStep::AgentTask {
                instruction,
                model_id,
                max_iterations,
                allowed_tools,
                temperature,
            } => {
                // AgentTask 在 Phase 2 中完整实现，Phase 1 返回占位
                self.execute_agent_task(
                    instruction,
                    model_id,
                    *max_iterations,
                    allowed_tools,
                    *temperature,
                    cancel_flag,
                )
                .await
            }
            ExecStep::Condition {
                expression,
                on_true,
                on_false,
            } => self.execute_condition(expression, on_true, on_false).await,
            ExecStep::Pipeline { name, params } => {
                self.execute_pipeline(name, params, event_tx, cancel_flag.clone())
                    .await
            }
        }
    }

    /// 执行 Pipeline（YAML 工作流）作为子 Plan
    /// 内联执行以避免递归 async fn
    pub(super) async fn execute_pipeline(
        &self,
        name: &str,
        _params: &std::collections::HashMap<String, String>,
        event_tx: mpsc::Sender<PlanProgressEvent>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        // 加载工作流
        let workflow = crate::agent::orchestrator::pipeline_adapter::PipelineAdapter::load_by_name(name)?;

        // 转换为 ExecutionPlan
        let vars = _params.clone();
        let sub_plan = crate::agent::orchestrator::pipeline_adapter::PipelineAdapter::convert(
            &workflow,
            "sub",
            &vars,
        );

        if sub_plan.steps.is_empty() {
            return Ok(Value::String(format!("Pipeline '{}' has no steps", name)));
        }

        // 内联执行子 Plan 步骤（不通过 execute_step 以免递归）
        let plan_id = sub_plan.id.clone();
        let total_steps = sub_plan.steps.len();
        let mut results: Vec<Value> = Vec::new();

        for (i, step) in sub_plan.steps.iter().enumerate() {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(ExecutionError::Cancelled);
            }

            let _ = event_tx
                .send(PlanProgressEvent {
                    plan_id: plan_id.clone(),
                    session_id: String::new(),
                    event_type: "step_started".to_string(),
                    step_index: Some(i),
                    step_label: Some(format!("[{}] {}", plan_id, step.label)),
                    result_summary: None,
                    error: None,
                    total_steps,
                    completed_steps: results.len(),
                })
                .await;

            // 直接调度步骤类型，不调用 execute_step
            let result = match &step.execution {
                ExecStep::ToolCall { tool, params, retry, timeout_seconds } => {
                    self.execute_tool(tool, params, retry, *timeout_seconds, cancel_flag.clone()).await
                }
                ExecStep::LlmCall { prompt, system_prompt, model_id, temperature, max_tokens } => {
                    self.execute_llm(prompt, system_prompt, model_id, *temperature, *max_tokens).await
                }
                ExecStep::AgentTask { instruction, model_id, max_iterations, allowed_tools, temperature } => {
                    self.execute_agent_task(instruction, model_id, *max_iterations, allowed_tools, *temperature, cancel_flag.clone()).await
                }
                ExecStep::Condition { expression, on_true, on_false } => {
                    self.execute_condition(expression, on_true, on_false).await
                }
                // Prevent deep nesting: Pipeline steps in sub-plans are not supported
                ExecStep::Pipeline { .. } => {
                    Err(ExecutionError::Internal("Nested pipeline execution is not supported".to_string()))
                }
            };

            match result {
                Ok(val) => {
                    let _ = event_tx
                        .send(PlanProgressEvent {
                            plan_id: plan_id.clone(),
                            session_id: String::new(),
                            event_type: "step_completed".to_string(),
                            step_index: Some(i),
                            step_label: Some(step.label.clone()),
                            result_summary: val.as_str().map(|s| s[..s.len().min(100)].to_string()),
                            error: None,
                            total_steps,
                            completed_steps: results.len() + 1,
                        })
                        .await;
                    results.push(val);
                }
                Err(e) => {
                    let _ = event_tx
                        .send(PlanProgressEvent {
                            plan_id: plan_id.clone(),
                            session_id: String::new(),
                            event_type: "step_failed".to_string(),
                            step_index: Some(i),
                            step_label: Some(step.label.clone()),
                            result_summary: None,
                            error: Some(e.to_string()),
                            total_steps,
                            completed_steps: results.len(),
                        })
                        .await;
                    return Err(e);
                }
            }
        }

        Ok(serde_json::json!({
            "pipeline": name,
            "steps_completed": results.len(),
            "results": results,
        }))
    }

    pub(super) async fn execute_tool(
        &self,
        tool: &str,
        params: &std::collections::HashMap<String, Value>,
        retry: &Option<RetryConfig>,
        _timeout_seconds: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        let max_retries = retry.as_ref().map(|r| r.max).unwrap_or(1);
        let delay = retry.as_ref().map(|r| r.delay_seconds).unwrap_or(0);

        let map: serde_json::Map<String, Value> = params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let input = Value::Object(map);

        for attempt in 0..max_retries {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(ExecutionError::Cancelled);
            }

            let tools = self.tools.lock().await;
            let result = tools.execute(tool, input.clone()).await;
            drop(tools);

            match result {
                Ok(val) => return Ok(val),
                Err(_e) if attempt < max_retries - 1 => {
                    if delay > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    }
                    continue;
                }
                Err(e) => {
                    return Err(ExecutionError::StepFailed {
                        step: 0,
                        message: format!(
                            "Tool '{}' failed after {} retries: {}",
                            tool,
                            max_retries,
                            e
                        ),
                    });
                }
            }
        }
        Err(ExecutionError::MaxRetries { step: 0 })
    }

    pub(super) async fn execute_llm(
        &self,
        prompt: &str,
        system_prompt: &Option<String>,
        model_id: &Option<String>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<Value, ExecutionError> {
        let providers = self.providers.lock().await;
        let mid = model_id.as_deref().unwrap_or_else(|| providers.default_model_id());
        if mid.is_empty() {
            return Err(ExecutionError::Internal("No model ID configured for LLM call".to_string()));
        }
        let provider = providers.get(mid).map_err(|e| {
            ExecutionError::Internal(format!("Failed to get provider for '{}': {}", mid, e))
        })?;

        let _ = (temperature, max_tokens); // reserved for future ProviderBox API

        let preamble = system_prompt.as_deref().unwrap_or("");
        let content = provider.prompt(preamble, prompt).await.map_err(|e| {
            ExecutionError::StepFailed {
                step: 0,
                message: format!("LLM call failed: {}", e),
            }
        })?;

        Ok(Value::String(content))
    }

    pub(super) async fn execute_agent_task(
        &self,
        instruction: &str,
        model_id: &Option<String>,
        max_iterations: Option<usize>,
        allowed_tools: &Option<Vec<String>>,
        _temperature: Option<f32>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        self.run_agent(instruction, None, model_id, max_iterations, allowed_tools, _temperature, cancel_flag)
            .await
    }

    /// 执行 AgentTask，失败时自动重试一次（带错误上下文）
    pub(super) async fn run_agent(
        &self,
        instruction: &str,
        prev_error: Option<&str>,
        model_id: &Option<String>,
        max_iterations: Option<usize>,
        allowed_tools: &Option<Vec<String>>,
        _temperature: Option<f32>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        let _ = _temperature; // reserved for future use

        // Retry loop: attempt 1, then retry once on failure
        let mut last_error: Option<String> = prev_error.map(|s| s.to_string());
        let max_attempts: usize = if prev_error.is_some() { 1 } else { 2 };

        for attempt in 0..max_attempts {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(ExecutionError::Cancelled);
            }

            let providers = self.providers.lock().await;
            let mid = model_id.as_deref().unwrap_or_else(|| providers.default_model_id());
            let mid = mid.to_string();
            drop(providers);

            if mid.is_empty() {
                return Err(ExecutionError::Internal("No model ID configured for agent task".to_string()));
            }

            // Build instruction with optional error context for retry
            let final_instruction = if let Some(err) = &last_error {
                format!(
                    "{}\n\nNote: A previous attempt failed with this error:\n{}\n\nPlease try a different approach to avoid the same issue.",
                    instruction, err
                )
            } else {
                instruction.to_string()
            };

            let system_msg = "You are an autonomous agent executing a specific task. \
                Use the available tools to complete the task step by step. \
                When you are done, provide a clear summary of what you accomplished.";

            let messages = vec![
                Message::system(system_msg.to_string()),
                Message::user(final_instruction),
            ];

            let agent = ToolLoop::new(self.providers.clone(), self.tools.clone());
            let agent = if let Some(max_iter) = max_iterations {
                agent.with_max_iterations(max_iter)
            } else {
                agent
            };

            let tools_enabled = allowed_tools.is_some() || last_error.is_some();

            match agent.run(&mid, messages, tools_enabled, allowed_tools.clone()).await {
                Ok(content) => {
                    return Ok(Value::String(content));
                }
                Err(e) if attempt < max_attempts - 1 => {
                    log::warn!("Agent task attempt {} failed, retrying: {}", attempt + 1, e);
                    last_error = Some(e.to_string());
                    // Continue to next attempt
                }
                Err(e) => {
                    return Err(ExecutionError::StepFailed {
                        step: 0,
                        message: format!("Agent task failed after {} attempts: {}", attempt + 1, e),
                    });
                }
            }
        }

        Err(ExecutionError::MaxRetries { step: 0 })
    }

    pub(super) async fn execute_condition(
        &self,
        expression: &str,
        _on_true: &BranchTarget,
        _on_false: &BranchTarget,
    ) -> Result<Value, ExecutionError> {
        let trimmed = expression.trim().to_lowercase();
        let is_truthy = !expression.is_empty()
            && !matches!(trimmed.as_str(), "false" | "no" | "0" | "" | "null");
        Ok(Value::Bool(is_truthy))
    }
}
