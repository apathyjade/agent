//! 人工确认闸门。
//!
//! 审批是**同步等待**的：工具调用在 dispatch 边界暂停，直到前端弹窗给出
//! 允许 / 拒绝，或 120 秒超时。服务本身只负责通道与排队，判定逻辑在
//! [`crate::llm::provider::decide`]。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::llm::provider::{ApprovalGate, ToolPolicy};
use crate::core::ipc::StreamChunk;

/// 等待用户响应的上限。
pub const APPROVAL_TIMEOUT_SECS: u64 = 120;

/// 发给前端的一次审批请求。
#[derive(Debug, Clone, Serialize)]
pub struct ToolApprovalRequest {
    /// 审批 id（即工具调用 id）。
    pub id: String,
    /// 工具名。
    pub tool_name: String,
    /// 本次调用的参数。
    pub arguments: Value,
    /// 为什么需要确认。
    pub reason: String,
}

/// 持有待确认请求的审批服务。
pub struct ToolApprovalService {
    app: AppHandle,
    policy: ToolPolicy,
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl ToolApprovalService {
    /// 创建服务。
    pub fn new(app: AppHandle, policy: ToolPolicy) -> Self {
        Self {
            app,
            policy,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// 前端回复审批；找不到对应请求时返回 `false`（可能已超时）。
    pub fn respond(&self, id: &str, approved: bool) -> bool {
        let sender = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id);
        match sender {
            Some(sender) => sender.send(approved).is_ok(),
            None => false,
        }
    }
}

#[async_trait]
impl ApprovalGate for ToolApprovalService {
    fn policy(&self) -> &ToolPolicy {
        &self.policy
    }

    async fn request(
        &self,
        call_id: &str,
        tool_name: &str,
        arguments: &Value,
        reason: &str,
    ) -> bool {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.insert(call_id.to_string(), tx);
        }

        let _ = self.app.emit(
            "tool_approval_request",
            ToolApprovalRequest {
                id: call_id.to_string(),
                tool_name: tool_name.to_string(),
                arguments: arguments.clone(),
                reason: reason.to_string(),
            },
        );
        emit_phase(&self.app, "waiting_confirmation");

        let approved = match tokio::time::timeout(Duration::from_secs(APPROVAL_TIMEOUT_SECS), rx)
            .await
        {
            Ok(Ok(approved)) => approved,
            Ok(Err(_)) => false,
            Err(_) => {
                log::warn!(
                    "tool approval for '{}' timed out after {}s",
                    call_id,
                    APPROVAL_TIMEOUT_SECS
                );
                false
            }
        };
        emit_phase(&self.app, "thinking");

        // 超时 / 取消后清掉残留；`respond` 已经移除时这里是空操作。
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(call_id);

        approved
    }
}

/// 上报审批阶段，让前端在弹窗之外也有状态提示。
fn emit_phase(app: &AppHandle, phase: &str) {
    let _ = app.emit(
        "stream_chunk",
        StreamChunk {
            content: String::new(),
            done: false,
            tool_calls: None,
            phase: Some(phase.to_string()),
        },
    );
}
