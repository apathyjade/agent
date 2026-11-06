//! 每轮工具策略状态与 Rig hook。
//!
//! Rig 的 hook 是「执行前的唯一闸门」：审批、并行冲突判定、跳过原因都发生在
//! 这里。本模块持有**一轮**的状态（decisions 等），并由 `llm/rig.rs` 在每次
//! 请求时新建一份——跨轮复用会把上一轮的工具结果混进下一轮。

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use rig::agent::{
    AgentHook, DispatchAction, DispatchEvent, HookContext, InvalidToolCallAction,
    InvalidToolCallContext, OutcomeAction, OutcomeEvent,
};
use rig::completion::ToolAnnotations;
use serde_json::Value;

use crate::llm::provider::{
    decide, ApprovalDecision, ApprovalGate, ToolCallStatus, ToolMetaMap, ToolPolicy,
};

/// 一轮工具执行的策略状态。
#[derive(Debug, Default)]
pub struct RunPolicyState {
    /// 每个 call id 的终态。
    decisions: HashMap<String, ToolCallStatus>,
    /// 本轮已放行调用的 footprint 登记（供并行冲突判定）。
    ///
    /// 只记录**已放行**的调用：被拒绝的不占位，后续调用仍可执行。
    footprints: Vec<(String, rig::__core::tool::Touches)>,
}

impl RunPolicyState {
    /// 新建一份空状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次调用的终态；已有记录时不覆盖。
    ///
    /// 先写入的结论优先：审批在 dispatch 阶段就决定了「用户拒绝」，后续
    /// outcome 不应把它改写成普通的「执行失败」。
    pub fn record(&mut self, call_id: impl Into<String>, status: ToolCallStatus) {
        self.decisions.entry(call_id.into()).or_insert(status);
    }

    /// 无条件记录（策略层明确知道最终结论时使用）。
    pub fn set(&mut self, call_id: impl Into<String>, status: ToolCallStatus) {
        self.decisions.insert(call_id.into(), status);
    }

    /// 当前所有终态的快照。
    pub fn decisions(&self) -> HashMap<String, ToolCallStatus> {
        self.decisions.clone()
    }

    /// 登记一次已放行调用的 footprint。
    pub fn register_footprint(&mut self, call_id: impl Into<String>, footprint: rig::__core::tool::Touches) {
        self.footprints.push((call_id.into(), footprint));
    }

    /// 撤销登记（调用没有真的执行时）。
    pub fn forget_footprint(&mut self, call_id: &str) {
        self.footprints.retain(|(id, _)| id != call_id);
    }

    /// 与已登记调用是否冲突。
    pub fn conflicts(&self, footprint: &rig::__core::tool::Touches) -> bool {
        self.footprints
            .iter()
            .any(|(_, existing)| existing.conflicts_with(footprint))
    }
}

/// 共享的一轮策略状态。
pub type SharedPolicyState = Arc<Mutex<RunPolicyState>>;

/// 创建一份共享状态。
pub fn shared_state() -> SharedPolicyState {
    Arc::new(Mutex::new(RunPolicyState::new()))
}

/// 记录工具调用终态的 hook。
///
/// 审批与并行冲突（task 7/8）会在这同一个 hook 的 `on_dispatch` 里先写
/// `Rejected` / `PolicyDenied`；这里只负责「实际跑过」的调用。
pub struct ToolPolicyHook {
    state: SharedPolicyState,
    approval: Option<Arc<dyn ApprovalGate>>,
    meta: Arc<ToolMetaMap>,
}

impl ToolPolicyHook {
    /// 绑定一轮状态、审批通道与工具元数据。
    pub fn new(
        state: SharedPolicyState,
        approval: Option<Arc<dyn ApprovalGate>>,
        meta: Arc<ToolMetaMap>,
    ) -> Self {
        Self {
            state,
            approval,
            meta,
        }
    }
}

impl AgentHook for ToolPolicyHook {
    fn name(&self) -> Option<String> {
        Some("tool-policy".to_string())
    }

    fn on_dispatch(
        &self,
        _ctx: &HookContext,
        event: DispatchEvent<'_>,
    ) -> impl Future<Output = DispatchAction> + Send {
        let state = self.state.clone();
        let approval = self.approval.clone();
        let meta = self.meta.clone();
        let tool_name = event.tool_name().map(str::to_string);
        let call_id = event.call_id.map(|id| id.to_string());
        let raw_args = event.tool_args().map(str::to_string);

        async move {
            let (Some(tool_name), Some(call_id)) = (tool_name, call_id) else {
                return DispatchAction::Proceed;
            };

            let args: Value = raw_args
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or(Value::Null);

            let tool_meta = meta.get(&tool_name);
            let annotations = tool_meta
                .map(|m| m.annotations.clone())
                // 没有元数据时不能假定「危险」，否则未知工具会无条件弹窗。
                .unwrap_or_else(|| {
                    ToolAnnotations::default()
                        .with_read_only(true)
                        .with_destructive(false)
                        .with_open_world(false)
                });
            let footprint = tool_meta.and_then(|m| m.footprint.as_ref().and_then(|f| f(&args)));

            let policy: ToolPolicy = approval
                .as_ref()
                .map(|gate| gate.policy().clone())
                .unwrap_or_default();
            let decision = decide(&policy, &tool_name, &annotations, footprint.as_ref());

            match decision {
                ApprovalDecision::Deny(reason) => {
                    record(&state, &call_id, ToolCallStatus::PolicyDenied);
                    DispatchAction::skip(reason)
                }
                ApprovalDecision::Allow => {
                    if let Some(footprint) = footprint {
                        if !reserve(&state, &call_id, footprint) {
                            record(&state, &call_id, ToolCallStatus::PolicyDenied);
                            return DispatchAction::skip(
                                "Another tool call in this turn already touches the same target",
                            );
                        }
                    }
                    DispatchAction::Proceed
                }
                ApprovalDecision::Confirm => {
                    // 先占位再等待：两个并发调用若同时判定「无冲突」，后一个
                    // 会覆盖前一个的文件。占位与判定必须在同一临界区内完成。
                    if let Some(footprint) = &footprint {
                        if !reserve(&state, &call_id, footprint.clone()) {
                            record(&state, &call_id, ToolCallStatus::PolicyDenied);
                            return DispatchAction::skip(
                                "Another tool call in this turn already touches the same target",
                            );
                        }
                    }

                    let approved = match &approval {
                        Some(gate) => {
                            gate.request(
                                &call_id,
                                &tool_name,
                                &args,
                                "This tool call may modify data and needs confirmation",
                            )
                            .await
                        }
                        // 没有审批通道时不能把写入当成已确认。
                        None => false,
                    };

                    if approved {
                        DispatchAction::Proceed
                    } else {
                        if footprint.is_some() {
                            if let Ok(mut guard) = state.lock() {
                                guard.forget_footprint(&call_id);
                            }
                        }
                        record(&state, &call_id, ToolCallStatus::Rejected);
                        DispatchAction::skip(
                            "User rejected this tool call; do not retry it, choose another approach.",
                        )
                    }
                }
            }
        }
    }

    fn on_outcome(
        &self,
        _ctx: &HookContext,
        event: OutcomeEvent<'_>,
    ) -> impl Future<Output = OutcomeAction> + Send {
        let state = self.state.clone();
        async move {
            if let Some(call_id) = event.call_id {
                let status = match event.tool_result() {
                    Some(result) if result.is_error() => ToolCallStatus::Failed,
                    Some(_) => ToolCallStatus::Completed,
                    // 没有 tool result 说明调用在到达 handler 前就结束了。
                    None => match event.outcome {
                        Err(report) if report.kind == rig::ErrorKind::Denied => {
                            ToolCallStatus::PolicyDenied
                        }
                        Err(report) if report.kind == rig::ErrorKind::Cancelled => {
                            ToolCallStatus::Rejected
                        }
                        _ => ToolCallStatus::Failed,
                    },
                };

                if let Ok(mut guard) = state.lock() {
                    guard.record(call_id.to_string(), status);
                }
            }
            OutcomeAction::Proceed
        }
    }
}

/// 记录一次调用的终态。
fn record(state: &SharedPolicyState, call_id: &str, status: ToolCallStatus) {
    if let Ok(mut guard) = state.lock() {
        guard.set(call_id, status);
    }
}

/// 在同一临界区里判冲突 + 登记；冲突时返回 `false`。
fn reserve(
    state: &SharedPolicyState,
    call_id: &str,
    footprint: rig::__core::tool::Touches,
) -> bool {
    let Ok(mut guard) = state.lock() else {
        return false;
    };
    if guard.conflicts(&footprint) {
        return false;
    }
    guard.register_footprint(call_id, footprint);
    true
}

/// 参数畸形时带反馈重试的 hook。
///
/// Rig 默认会在参数不是 JSON 对象时直接失败整轮；对模型来说这是可修复的
/// 错误，应当把「哪里不对、应该怎么发」回灌给它，并允许有限次重试。
pub struct InvalidCallHook;

impl AgentHook for InvalidCallHook {
    fn name(&self) -> Option<String> {
        Some("invalid-tool-call".to_string())
    }

    fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> impl Future<Output = Option<InvalidToolCallAction>> + Send {
        let feedback = build_invalid_call_feedback(event);
        async move { Some(InvalidToolCallAction::retry(feedback)) }
    }
}

/// 把 Rig 的诊断整理成模型能直接照做的反馈。
fn build_invalid_call_feedback(event: &InvalidToolCallContext) -> String {
    let reason = match &event.reason {
        rig::run::policy::InvalidToolCallReason::MalformedArguments { error } => {
            format!("the arguments were not a JSON object ({})", error)
        }
        rig::run::policy::InvalidToolCallReason::UnknownTool => {
            "that tool name is not registered".to_string()
        }
        rig::run::policy::InvalidToolCallReason::DisallowedByToolChoice => {
            "that tool is not allowed by the current tool choice".to_string()
        }
        _ => "the call could not be dispatched".to_string(),
    };

    let available = if event.available_tools.is_empty() {
        "none".to_string()
    } else {
        event.available_tools.join(", ")
    };

    format!(
        "Your call to '{}' was rejected: {}. Send the call again with a single JSON object as its arguments. Available tools: {}.",
        event.tool_name, reason, available
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::run::policy::InvalidToolCallContext;

    /// 通过 serde 构造 Rig 的诊断结构（它标了 non_exhaustive，不能直接写字面量）。
    fn malformed_event() -> InvalidToolCallContext {
        serde_json::from_value(serde_json::json!({
            "tool_name": "calculator",
            "tool_call_id": null,
            "args": "not a json object",
            "available_tools": ["calculator", "file_system"],
            "allowed_tools": [],
            "tool_choice": null,
            "chat_history": [],
            "is_streaming": false,
            "reason": {
                "reason": "malformed_arguments",
                "error": "expected a JSON object"
            }
        }))
        .expect("诊断结构应能反序列化")
    }

    #[test]
    fn invalid_call_feedback_names_format_and_available_tools() {
        let feedback = build_invalid_call_feedback(&malformed_event());

        assert!(
            feedback.contains("single JSON object"),
            "反馈应指明参数必须是 JSON 对象: {}",
            feedback
        );
        assert!(feedback.contains("calculator"), "反馈应列出可用工具");
        assert!(feedback.contains("file_system"), "反馈应列出可用工具");
    }

    #[test]
    fn decisions_first_write_wins() {
        let mut state = RunPolicyState::new();

        // dispatch 阶段先判定「用户拒绝」，outcome 不应把它改写成普通失败。
        state.record("call_1", ToolCallStatus::Rejected);
        state.record("call_1", ToolCallStatus::Completed);

        assert_eq!(
            state.decisions().get("call_1"),
            Some(&ToolCallStatus::Rejected)
        );
    }
}
