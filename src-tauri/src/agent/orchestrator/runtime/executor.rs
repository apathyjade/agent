//! `ExecutionRuntime` 的单步执行实现。
//!
//! 计划编排（`run`）与持久化辅助留在 `runtime.rs`；本模块只负责按步骤类型
//! 分派：工具调用、LLM 调用、agent 任务、条件判断、子 Pipeline。

use super::*;
use crate::core::error::AppError;

/// `AgentTask` 未指定系统提示词时使用的内置文案。
const DEFAULT_AGENT_TASK_SYSTEM_PROMPT: &str = "You are an autonomous agent executing a specific task. \
    Use the available tools to complete the task step by step. \
    When you are done, provide a clear summary of what you accomplished.";

impl ExecutionRuntime {
    /// 执行单个 ExecStep。
    ///
    /// Pipeline 步骤单独处理，其余步骤交给 [`Self::execute_sub_step`]，
    /// 这样顶层步骤与子计划步骤共享同一套调度逻辑。
    pub(super) async fn execute_step(
        &self,
        step: &ExecStep,
        _step_id: &str,
        cancel_flag: Arc<AtomicBool>,
        event_tx: mpsc::Sender<PlanProgressEvent>,
    ) -> Result<Value, ExecutionError> {
        match step {
            ExecStep::Pipeline { name, params } => {
                self.execute_pipeline(name, params, event_tx, cancel_flag.clone())
                    .await
            }
            other => self.execute_sub_step(other, cancel_flag).await,
        }
    }

    /// 调度一个非 Pipeline 步骤。
    ///
    /// 子计划同样走这里，因此 Pipeline 步骤在此直接报错，避免无限递归。
    async fn execute_sub_step(
        &self,
        step: &ExecStep,
        cancel_flag: Arc<AtomicBool>,
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
            // 防止深层嵌套：子计划不支持 Pipeline 步骤。
            ExecStep::Pipeline { .. } => Err(ExecutionError::Internal(
                "Nested pipeline execution is not supported".to_string(),
            )),
        }
    }

    /// 执行 Pipeline（YAML 工作流）作为子 Plan。
    ///
    /// 内联执行子计划的步骤，而不是再次调用 [`Self::execute_step`]，
    /// 以免 async 函数递归。
    pub(super) async fn execute_pipeline(
        &self,
        name: &str,
        params: &std::collections::HashMap<String, String>,
        event_tx: mpsc::Sender<PlanProgressEvent>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        let workflow =
            crate::agent::orchestrator::pipeline_adapter::PipelineAdapter::load_by_name(name)?;

        // 把调用参数作为模板变量传给子计划。
        let vars = params.clone();
        let sub_plan = crate::agent::orchestrator::pipeline_adapter::PipelineAdapter::convert(
            &workflow,
            "sub",
            &vars,
        );

        if sub_plan.steps.is_empty() {
            return Ok(Value::String(format!("Pipeline '{}' has no steps", name)));
        }

        let plan_id = sub_plan.id.clone();
        let total_steps = sub_plan.steps.len();
        let mut results: Vec<Value> = Vec::new();

        for (i, step) in sub_plan.steps.iter().enumerate() {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(ExecutionError::Cancelled);
            }

            Self::send_progress(
                &event_tx,
                PlanProgressEvent::new(
                    plan_id.as_str(),
                    String::new(),
                    "step_started",
                    total_steps,
                    results.len(),
                )
                .with_step_index(i)
                .with_step_label(format!("[{}] {}", plan_id, step.label)),
            )
            .await;

            let result = self
                .execute_sub_step(&step.execution, cancel_flag.clone())
                .await;

            match result {
                Ok(val) => {
                    let mut event = PlanProgressEvent::new(
                        plan_id.as_str(),
                        String::new(),
                        "step_completed",
                        total_steps,
                        results.len() + 1,
                    )
                    .with_step_index(i)
                    .with_step_label(step.label.clone());
                    if let Some(summary) = result_summary(&val) {
                        event = event.with_result_summary(summary);
                    }
                    Self::send_progress(&event_tx, event).await;
                    results.push(val);
                }
                Err(e) => {
                    Self::send_progress(
                        &event_tx,
                        PlanProgressEvent::new(
                            plan_id.as_str(),
                            String::new(),
                            "step_failed",
                            total_steps,
                            results.len(),
                        )
                        .with_step_index(i)
                        .with_step_label(step.label.clone())
                        .with_error(e.to_string()),
                    )
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

    /// 执行一次工具调用，按 `RetryConfig` 重试。
    ///
    /// `timeout_seconds` 到期时该步骤以超时失败结束：超时意味着工具已经卡住，
    /// 重试只会把同一个卡死再走一遍。
    pub(super) async fn execute_tool(
        &self,
        tool: &str,
        params: &std::collections::HashMap<String, Value>,
        retry: &Option<RetryConfig>,
        timeout_seconds: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        // 先做策略校验，避免无谓地触碰工具注册表。
        self.validate_tool_call(tool)?;

        let max_retries = retry.as_ref().map(|r| r.max).unwrap_or(1);
        let delay = retry.as_ref().map(|r| r.delay_seconds).unwrap_or(0);

        let map: serde_json::Map<String, Value> =
            params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let input = Value::Object(map);

        for attempt in 0..max_retries {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(ExecutionError::Cancelled);
            }

            let call = async {
                let tools = self.tools.lock().await;
                tools.execute(tool, input.clone()).await
            };

            let result = match timeout_seconds.filter(|secs| *secs > 0) {
                Some(secs) => {
                    match tokio::time::timeout(tokio::time::Duration::from_secs(secs), call).await
                    {
                        Ok(result) => result,
                        Err(_) => {
                            return Err(ExecutionError::StepFailed {
                                step: 0,
                                message: format!(
                                    "Tool '{}' timed out after {} seconds",
                                    tool, secs
                                ),
                            });
                        }
                    }
                }
                None => call.await,
            };

            match result {
                Ok(val) => return Ok(val),
                Err(AppError::ApprovalDenied(reason)) => {
                    return Err(ExecutionError::ApprovalDenied { reason });
                }
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
                            tool, max_retries, e
                        ),
                    });
                }
            }
        }
        Err(ExecutionError::MaxRetries { step: 0 })
    }

    /// 执行一次纯文本 LLM 调用，返回模型输出。
    pub(super) async fn execute_llm(
        &self,
        prompt: &str,
        system_prompt: &Option<String>,
        model_id: &Option<String>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<Value, ExecutionError> {
        let providers = self.providers.lock().await;
        let mid = model_id
            .as_deref()
            .unwrap_or_else(|| providers.default_model_id());
        if mid.is_empty() {
            return Err(ExecutionError::Internal(
                "No model ID configured for LLM call".to_string(),
            ));
        }
        let provider = providers.get(mid).map_err(|e| {
            ExecutionError::Internal(format!("Failed to get provider for '{}': {}", mid, e))
        })?;

        // 采样参数暂时保留，ProviderBox 尚未暴露对应接口。
        let _ = (temperature, max_tokens);

        let preamble = system_prompt.as_deref().unwrap_or("");
        let content = provider
            .prompt(preamble, prompt)
            .await
            .map_err(|e| ExecutionError::StepFailed {
                step: 0,
                message: format!("LLM call failed: {}", e),
            })?;

        Ok(Value::String(content))
    }

    /// 执行一个 `AgentTask` 步骤：先套用运行级策略，再进入带重试的 agent 循环。
    pub(super) async fn execute_agent_task(
        &self,
        instruction: &str,
        model_id: &Option<String>,
        max_iterations: Option<usize>,
        allowed_tools: &Option<Vec<String>>,
        _temperature: Option<f32>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        let effective_allowed_tools = self.effective_allowed_tools(allowed_tools);
        self.validate_agent_task(&effective_allowed_tools)?;
        let effective_max_iterations = self.effective_max_iterations(max_iterations);

        self.run_agent(
            instruction,
            None,
            model_id,
            effective_max_iterations,
            &effective_allowed_tools,
            _temperature,
            cancel_flag,
        )
        .await
    }

    /// 执行 AgentTask，失败时自动重试一次（重试会带上上一次的错误上下文）。
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
        // 采样参数暂时保留，ProviderBox 尚未暴露对应接口。
        let _ = _temperature;

        // 重试循环：首次尝试，失败后再试一次。
        let mut last_error: Option<String> = prev_error.map(|s| s.to_string());
        let max_attempts: usize = if prev_error.is_some() { 1 } else { 2 };

        for attempt in 0..max_attempts {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(ExecutionError::Cancelled);
            }

            let providers = self.providers.lock().await;
            let mid = model_id
                .as_deref()
                .unwrap_or_else(|| providers.default_model_id());
            let mid = mid.to_string();
            drop(providers);

            if mid.is_empty() {
                return Err(ExecutionError::Internal(
                    "No model ID configured for agent task".to_string(),
                ));
            }

            // 重试时把上一次的错误拼进指令，提示模型换一种做法。
            let final_instruction = if let Some(err) = &last_error {
                format!(
                    "{}\n\nNote: A previous attempt failed with this error:\n{}\n\nPlease try a different approach to avoid the same issue.",
                    instruction, err
                )
            } else {
                instruction.to_string()
            };

            let messages = self.agent_task_messages(&final_instruction);

            let agent = ToolLoop::new(self.providers.clone(), self.tools.clone());
            let agent = if let Some(max_iter) = max_iterations {
                agent.with_max_iterations(max_iter)
            } else {
                agent
            };

            // `None` 表示「不限制工具」，不是「不给工具」——只有运行级开关
            // 才能真正关掉工具。白名单为空时 `to_rig_tools` 自己会返回空集。
            let tools_enabled = self.tools_enabled;

            match agent
                .run(&mid, messages, tools_enabled, allowed_tools.clone())
                .await
            {
                Ok(outcome) => {
                    return Ok(Value::String(outcome.output));
                }
                Err(e) if attempt < max_attempts - 1 => {
                    log::warn!("Agent task attempt {} failed, retrying: {}", attempt + 1, e);
                    last_error = Some(e.to_string());
                    // 继续下一次尝试。
                }
                Err(e) => {
                    return Err(ExecutionError::StepFailed {
                        step: 0,
                        message: format!(
                            "Agent task failed after {} attempts: {}",
                            attempt + 1,
                            e
                        ),
                    });
                }
            }
        }

        Err(ExecutionError::MaxRetries { step: 0 })
    }

    /// 构造 agent 任务的消息列表。
    ///
    /// 抽成独立方法是为了让测试能在没有可用 provider 的情况下断言系统提示词；
    /// 覆盖值为空时回落到内置提示词。
    pub(super) fn agent_task_messages(&self, instruction: &str) -> Vec<Message> {
        let system_msg = self
            .system_prompt
            .as_deref()
            .filter(|prompt| !prompt.trim().is_empty())
            .unwrap_or(DEFAULT_AGENT_TASK_SYSTEM_PROMPT);

        vec![
            Message::system(system_msg.to_string()),
            Message::user(instruction.to_string()),
        ]
    }

    /// 求值一个条件表达式。
    ///
    /// 目前只做「真值判断」：空串、`false`、`no`、`0`、`null` 视为假，
    /// 其余非空表达式为真；分支目标由调用方决定。
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
