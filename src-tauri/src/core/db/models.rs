//! 数据库层的领域模型。
//!
//! 这些结构体同时用于数据库读写与 IPC 序列化，因此字段名即对外契约：
//! 重命名会破坏前端与已持久化的数据，只能新增或弃用。

use serde::{Deserialize, Serialize};

/// 虚拟人格：一个有名字、独立系统提示词、记忆与工具配置的身份。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaRecord {
    pub id: String,
    /// 唯一名称，供按名查找使用。
    pub name: String,
    /// 展示用副标题（如职位），可为空。
    pub title: String,
    /// 头像 emoji。
    pub emoji: String,
    /// 简介。
    pub description: String,
    /// 该人格的系统提示词正文。
    pub system_prompt: String,
    /// 采样温度，默认 0.3。
    pub temperature: f64,
    /// 回复风格标记（如 "concise"）；语义由上层定义。
    pub response_style: String,
    /// 绑定的模型提供方；空串表示沿用会话或全局设置。
    pub model_provider: String,
    /// 绑定的模型名；空串表示沿用会话或全局设置。
    pub model_name: String,
    /// 是否为全局默认人格；同一时刻至多一个为真（由写入方维护）。
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 项目：磁盘上一个有名字的目录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// 目录路径；不保证在磁盘上仍然存在。
    pub path: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 会话：一串消息及其标题、模型与可选系统提示词。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub model_id: String,
    pub system_prompt: Option<String>,
    /// 绑定到本会话的人格；`None` 表示不绑定。
    pub persona_id: Option<String>,
    /// 会话级配置的 JSON 字符串（如 enabled_tools）；`None` 表示使用默认配置。
    pub config: Option<String>,
    /// 标题来源，合法值为 "manual"（用户手改）与 "auto_generated"（模型生成）。
    /// 仅用于展示与统计，不影响标题本身。
    pub title_source: String,
    /// 是否已归档；归档会话仍可被列表接口读到。
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    /// 会话模式，合法值："chat"（普通对话）与 "autonomous"（自主执行）。
    #[serde(default)]
    pub mode: String,
    /// 执行状态，JSON 序列化的 ExecStatus（如 `{"type":"idle"}`）；
    /// 由执行运行时写入，数据库层不解析。
    #[serde(default)]
    pub execution_status: String,
    /// 当前活跃执行计划 id；`None` 表示没有进行中的计划。
    #[serde(default)]
    pub active_plan_id: Option<String>,
    /// 会话归属的项目；`None` 表示未归属任何项目。
    #[serde(default)]
    pub project_id: Option<String>,
    /// 会话最近所属的意图领域。`auto` 模式下它只是路由先验，
    /// 不限制消息可以落到哪个领域；`None` 表示没有先验。
    #[serde(default)]
    pub intent_domain_id: Option<String>,
    /// 会话领域模式，只有两个合法值：
    /// - "auto"：`intent_domain_id` 仅作为路由先验，消息可落到任意领域；
    /// - "pinned"：会话被钉在 `intent_domain_id` 指向的领域。
    ///
    /// 领域被删除时会回落到 "auto"。该字段刻意保持字符串而非枚举，
    /// 以维持与前端和数据库的契约。
    #[serde(default = "default_intent_domain_mode")]
    pub intent_domain_mode: String,
}

/// [`Session::intent_domain_mode`] 的默认值；读取旧版本持久化数据时同样适用。
fn default_intent_domain_mode() -> String {
    "auto".to_string()
}

/// 会话内一段消息区间的摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub session_id: String,
    pub message_start_id: String,
    pub message_end_id: String,
    pub summary: String,
    /// 要点列表的 JSON 数组字符串；`None` 表示未提取要点。
    pub key_points: Option<String>,
    pub original_token_count: i32,
    pub summary_token_count: i32,
    /// 生成摘要时使用的模型；`None` 表示未记录。
    pub model_used: Option<String>,
    pub created_at: String,
}

/// 一条会话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    /// 角色，如 "user" / "assistant" / "tool"。
    pub role: String,
    /// 消息正文。
    pub content: String,
    /// 工具调用的 JSON 字符串；`None` 表示本条没有工具调用。
    pub tool_calls: Option<String>,
    /// 本消息对应的工具调用 id（`role = "tool"` 时使用）。
    pub tool_call_id: Option<String>,
    /// 工具消息对应的真实工具名；旧数据为 `None`，读取时回退到占位名。
    #[serde(default)]
    pub tool_name: Option<String>,
    /// 该消息的 token 数；`None` 表示未统计。
    pub tokens: Option<i32>,
    pub created_at: String,
}

/// 键值设置项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    /// 字符串值；需要结构化数据时由调用方自行解析（如 JSON）。
    pub value: String,
}

/// 可复用的系统提示词模板。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPrompt {
    pub id: String,
    pub name: String,
    pub content: String,
    /// 是否为全局默认提示词；同一时刻至多一个为真。
    pub is_default: bool,
    pub created_at: String,
}

/// 已绑定到应用的本地项目目录（用于项目环境扫描）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundProjectModel {
    pub id: String,
    /// 目录路径；在库中唯一。
    pub path: String,
    pub name: String,
    /// 是否在启动或切换项目时自动扫描。
    pub auto_sync: bool,
    /// 最近一次扫描时间（RFC 3339）；`None` 表示从未扫描。
    pub last_scan: Option<String>,
    /// 项目依赖需求的 JSON 字符串；`None` 表示尚未解析。
    pub requirements: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 某运行时（如 node、python）的可用版本缓存条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeVersionCache {
    /// 运行时类型标识。
    pub runtime_type: String,
    /// 版本号字符串；与 `runtime_type` 组合唯一。
    pub version: String,
    pub display_name: String,
    /// 安装包下载地址。
    pub url: String,
    /// LTS 代号（如 "iron"）；`None` 表示非 LTS。
    pub lts: Option<String>,
    /// 是否为稳定版。
    pub is_stable: bool,
    /// 官方发布日期字符串；格式随数据源而定。
    pub release_date: Option<String>,
    /// 安装包大小（字节）；`None` 表示未知。
    pub file_size: Option<i64>,
    /// 本条缓存最后一次从远端刷新的时间。
    pub fetched_at: String,
}

/// 代理记住的一条结构化记忆。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    /// 记忆正文。
    pub content: String,
    /// 分类："fact" / "preference" / "project_context" / "user_info" / "conversation_summary"。
    pub memory_type: String,
    /// 生效范围："global" / "conversation:<id>" / "project:<id>"。
    pub scope: String,
    /// 来源："manual" / "auto_extracted" / "conversation:<id>"。
    pub source: String,
    /// 相关度评分，0.0–1.0。
    pub relevance: f64,
    /// 逗号分隔的标签，用于简单分类。
    pub tags: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: String,
    /// 累计被访问次数。
    pub access_count: i32,
}

/// `execution_plans` 表的记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanRecord {
    pub id: String,
    pub session_id: String,
    /// 计划来源（如 "user" / "model"）；由上层定义，数据库层不校验。
    pub source: String,
    /// 计划目标描述；`None` 表示未提供。
    pub goal: Option<String>,
    /// 计划正文的 JSON 字符串（结构化步骤定义）。
    pub plan_json: String,
    /// 计划状态字符串（如 "pending" / "running" / "completed" / "failed"）；
    /// 由上层维护，数据库层不校验。
    pub status: String,
    pub created_at: String,
    /// 完成时间；`None` 表示尚未结束。
    pub finished_at: Option<String>,
}

/// `execution_plan_steps` 表的记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepRecord {
    pub id: String,
    pub plan_id: String,
    /// 步骤在计划中的序号（从 0 开始）；读取时按此升序。
    pub step_index: i32,
    pub label: String,
    /// 步骤类型标记；由上层定义，数据库层不校验。
    pub step_type: String,
    /// 步骤状态字符串（如 "pending" / "running" / "completed" / "failed"）；
    /// 由上层维护。
    pub status: String,
    /// 步骤结果的 JSON 字符串；`None` 表示无结果。
    pub result_json: Option<String>,
    /// 失败原因；`None` 表示未失败。
    pub error: Option<String>,
    /// 开始时间；`None` 表示尚未开始。
    pub started_at: Option<String>,
    /// 耗时（毫秒）；`None` 表示未统计。
    pub duration_ms: Option<i64>,
}

/// `workflow_runs` 表的记录：一次工作流执行的快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub id: String,
    pub workflow_name: String,
    /// 运行状态，合法值："running" / "completed" / "failed" / "cancelled"。
    pub status: String,
    /// 各步骤结果的 JSON 对象（HashMap<String, Value>）。
    pub step_results: Option<String>,
    /// 步骤进度的 JSON 字符串；`None` 表示未上报。
    pub step_progress: Option<String>,
    /// 失败原因；`None` 表示没有错误。
    pub error: Option<String>,
    /// 触发方式，合法值："manual"（手动）与 "scheduled"（定时）。
    pub trigger_type: String,
    pub started_at: String,
    /// 结束时间；`None` 表示仍在运行。
    pub finished_at: Option<String>,
}

/// `intent_domains` 表的记录：路由的第一层分类。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentDomainRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 是否为当前活跃领域。自会话拥有独立领域状态后，它也充当**新会话的默认领域**。
    ///
    /// 注意它与 [`IntentRecord::enabled`] 含义不同：`is_active` 表示“整个领域
    /// 当前被选中”，`enabled` 表示“单个意图是否参与路由”，两者互不推导。
    pub is_active: bool,
    /// 领域来源（如 "user" 表示用户创建，或内置种子）；数据库层不校验。
    pub source: String,
    /// 领域结构版本号，导入与升级时用于比对。
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// `intents` 表的记录：领域内的一条具体处理路径。
///
/// `examples` 与 `allowed_tools` 在 SQLite 中以 JSON 文本存储。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRecord {
    pub id: String,
    pub domain_id: String,
    /// 领域内唯一的名称。
    pub name: String,
    pub description: String,
    /// 触发示例句子，落库为 JSON 数组文本。
    pub examples: Vec<String>,
    /// 关联的工作流标识；空串表示不使用工作流。
    pub workflow: String,
    /// 该意图允许调用的工具名列表，落库为 JSON 数组文本。
    pub allowed_tools: Vec<String>,
    /// 默认执行 agent；空串表示沿用全局默认。
    pub default_agent: String,
    /// 生效范围（如 "global"）；语义由上层定义。
    pub scope: String,
    /// 是否参与路由。与 [`IntentDomainRecord::is_active`] 不同：
    /// 后者指整个领域被选中，这里指单条意图是否可用。
    pub enabled: bool,
    /// 意图版本号；每次更新由数据库层 +1。
    pub version: i64,
    /// 分类置信度阈值（0.0–1.0）：判定低于该值时走 `fallback_intent`。
    pub confidence_threshold: f64,
    /// 置信度不足时回退到的意图 id；空串表示回退到澄清（clarify）。
    /// 删除意图时数据库层会清空指向它的引用。
    pub fallback_intent: String,
    /// 路由路径标记（如 "fast"）；由上层定义。
    pub path: String,
    /// 置信度不足时是否自动升级到更重的处理路径，而不是直接回退。
    pub auto_escalate: bool,
    /// 命中该意图时追加到系统提示词末尾的文本。
    pub system_prompt_appendix: String,
    /// 该意图指定的模型；空串表示沿用会话模型。
    pub model_id: String,
    /// 该意图允许的最大迭代轮数；`None` 表示沿用全局默认。
    pub max_iterations: Option<usize>,
    /// 领域内的展示与路由顺序，越小越靠前；读取时按此升序。
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session() -> Session {
        Session {
            id: "session-a".to_string(),
            title: "测试会话".to_string(),
            model_id: "model-a".to_string(),
            system_prompt: None,
            persona_id: None,
            config: None,
            title_source: "manual".to_string(),
            archived: false,
            created_at: "2026-11-05T00:00:00Z".to_string(),
            updated_at: "2026-11-05T00:00:00Z".to_string(),
            mode: "chat".to_string(),
            execution_status: r#"{"type":"idle"}"#.to_string(),
            active_plan_id: None,
            project_id: None,
            intent_domain_id: Some("domain-a".to_string()),
            intent_domain_mode: "pinned".to_string(),
        }
    }

    #[test]
    fn session_json_round_trips_intent_domain_fields() {
        let session = sample_session();
        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.intent_domain_id.as_deref(), Some("domain-a"));
        assert_eq!(restored.intent_domain_mode, "pinned");
        assert_eq!(restored.id, session.id);
    }

    #[test]
    fn session_json_without_intent_fields_still_deserializes() {
        let mut value = serde_json::to_value(sample_session()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("intent_domain_id");
        object.remove("intent_domain_mode");

        let restored: Session = serde_json::from_value(value).unwrap();

        assert!(
            restored.intent_domain_id.is_none(),
            "缺少 intent_domain_id 的旧 JSON 必须默认为 None"
        );
        assert_eq!(
            restored.intent_domain_mode, "auto",
            "缺少 intent_domain_mode 的旧 JSON 必须默认为 auto"
        );
    }
}
