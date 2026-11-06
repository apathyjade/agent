//! 工作流定义的序列化模型。
//!
//! 这些类型直接对应 YAML 结构：`trigger` 与 `steps` 用 `type` 字段做标签
//! 区分，缺省值由 serde 默认函数补齐，使最小配置也能解析。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 工作流的触发方式。
///
/// 缺省为手动触发；`cron` 与 `file_watch` 目前只做描述，调度由外部完成。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDef {
    /// 手动运行。
    #[default]
    Manual,
    /// 按 cron 表达式定时触发。
    Cron { schedule: String },
    /// 监听指定路径的文件变化触发。
    FileWatch { path: String, pattern: String },
}

/// 一条工作流定义，对应一个 YAML 文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    /// 工作流名称，作为运行记录里的标识。
    pub name: String,
    /// 面向用户的说明，缺省为空。
    #[serde(default)]
    pub description: String,
    /// 触发方式，缺省为手动。
    #[serde(default)]
    pub trigger: TriggerDef,
    /// 顺序执行的步骤列表。
    #[serde(default)]
    pub steps: Vec<StepDef>,
}

/// 单个步骤的定义，用 `type` 字段区分三类步骤。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDef {
    /// 调用一个已注册的工具。
    ToolCall {
        /// 步骤 id，用于后续步骤引用其结果。
        id: String,
        /// 工具名。
        tool: String,
        /// 工具参数，字符串值支持模板渲染。
        #[serde(default)]
        params: HashMap<String, Value>,
        /// 重试配置；缺省表示只执行一次。
        #[serde(default)]
        retry: Option<RetryConfig>,
        /// 重试耗尽后的行为：`fail`（默认）或 `skip`。
        #[serde(default = "default_on_error")]
        on_error: String,
        /// 单次尝试的超时秒数；缺省不设超时。
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    /// 调用 LLM 生成一段文本。
    LlmCall {
        /// 步骤 id。
        id: String,
        /// 用户提示词，支持模板渲染。
        #[serde(default)]
        prompt: String,
        /// 模型 id；缺省使用 provider 注册表的默认模型。
        #[serde(default)]
        model_id: Option<String>,
        /// 系统提示词。
        #[serde(default)]
        system_prompt: Option<String>,
        /// 生成 token 上限（当前引擎未使用）。
        #[serde(default)]
        max_tokens: Option<u32>,
        /// 采样温度（当前引擎未使用）。
        #[serde(default)]
        temperature: Option<f32>,
        /// 重试配置。
        #[serde(default)]
        retry: Option<RetryConfig>,
        /// 重试耗尽后的行为。
        #[serde(default = "default_on_error")]
        on_error: String,
        /// 单次尝试的超时秒数。
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    /// 计算一个布尔条件，可选提前结束工作流。
    Condition {
        /// 步骤 id。
        id: String,
        /// 条件表达式，渲染后按真假值字符串判断。
        condition: String,
        /// 条件为假时的行为：`end`（默认）结束工作流，其他值继续。
        #[serde(default = "default_on_false")]
        on_false: String,
    },
}

/// 步骤的重试策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// 最大尝试次数。
    pub max: u32,
    /// 两次尝试之间的等待秒数。
    #[serde(default = "default_retry_delay")]
    pub delay_seconds: u64,
}

fn default_retry_delay() -> u64 {
    3
}
fn default_on_error() -> String {
    "fail".to_string()
}
fn default_on_false() -> String {
    "end".to_string()
}

/// 推送给前端的单步执行进度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepProgress {
    /// 步骤 id。
    pub step_id: String,
    /// 状态字符串：`running` / `completed` / `failed`。
    pub status: String,
    /// 已耗时毫秒数，运行中为空。
    pub duration_ms: Option<u64>,
    /// 失败原因。
    pub error: Option<String>,
    /// 结果摘要，仅成功时有值。
    pub result_summary: Option<String>,
}

pub use crate::core::db::models::WorkflowRunRecord;

/// 前端工作流列表项，聚合定义与最近一次运行信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    /// 工作流名称。
    pub name: String,
    /// 工作流说明。
    pub description: String,
    /// 步骤数量。
    pub step_count: usize,
    /// YAML 文件路径。
    pub file_path: String,
    /// 触发方式的展示文本。
    pub trigger: String,
    /// 下次运行时间（当前未计算）。
    pub next_run_at: Option<String>,
    /// 最近一次运行状态。
    pub last_run_status: Option<String>,
    /// 最近一次运行时间。
    pub last_run_at: Option<String>,
}

impl WorkflowDef {
    /// 渲染 `{{ steps.<id>.result }}`、`{{ vars.<key> }}`、
    /// `{{ secrets.<key> }}` 三类占位符。
    ///
    /// 只做字面替换：未知占位符原样保留，值按字符串直接拼接，不做转义。
    pub fn render_template(
        template: &str,
        step_results: &HashMap<String, Value>,
        vars: &HashMap<String, String>,
        secrets: &HashMap<String, String>,
    ) -> String {
        let mut result = template.to_string();
        for (step_id, value) in step_results {
            let placeholder = format!("{{{{ steps.{}.result }}}}", step_id);
            let rendered = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &rendered);
        }
        for (key, val) in vars {
            let placeholder = format!("{{{{ vars.{} }}}}", key);
            result = result.replace(&placeholder, val);
        }
        for (key, val) in secrets {
            let placeholder = format!("{{{{ secrets.{} }}}}", key);
            result = result.replace(&placeholder, val);
        }
        result
    }
}
