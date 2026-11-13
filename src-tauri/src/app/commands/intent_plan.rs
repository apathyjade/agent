//! 会话级计划：拆解结果 → 执行计划 → 用户确认 → 顺序执行。
//!
//! 本模块负责确认卡所需的后端能力：
//! - [`decompose_message`]：拆解一条消息并落一条 `pending` 计划（无任何副作用）；
//! - [`update_plan_step`] / [`remove_plan_step`] / [`cancel_plan`]：确认卡编辑操作；
//! - 把拆解结果组装成 [`ExecutionPlan`]（工具白名单取交集，deep 意图展开为子步骤）。
//!
//! 「确认前无副作用」是硬约束：本模块只写计划与步骤记录，从不执行步骤。

use serde::Serialize;
use tauri::State;

use crate::agent::orchestrator::plan_types::{
    exec_step_type_name, ExecutionPlan, ExecStep, PlanSource, PlanStatus, PlanStep, StepStatus,
};
use crate::agent::orchestrator::planner::LlmPlanner;
use crate::app::commands::session::{emit_intent_resolved, session_enabled_tools, zero_decomposition};
use crate::app::state::AppState;
use crate::core::db::models::{ExecutionPlanRecord, PlanStepRecord};
use crate::core::db::repository::Database;
use crate::core::error::{AppError, Result};
use crate::llm::intent::router::{intersect_tools, IntentRouter};
use crate::llm::intent::{
    DecisionEvidence, DecisionSource, DecomposedIntent, Decomposition, IntentPolicy,
    IntentResult, PathSelection, ScopeStatus,
};

/// 深意图展开为子步骤的边界；可在测试中注入替身。
#[async_trait::async_trait]
pub trait DeepExpander: Send + Sync {
    async fn expand(
        &self,
        instruction: &str,
        session_id: &str,
        model_id: Option<&str>,
        allowed_tools: Option<&[String]>,
        tools_enabled: bool,
    ) -> std::result::Result<ExecutionPlan, String>;
}

#[async_trait::async_trait]
impl DeepExpander for LlmPlanner {
    async fn expand(
        &self,
        instruction: &str,
        session_id: &str,
        model_id: Option<&str>,
        allowed_tools: Option<&[String]>,
        tools_enabled: bool,
    ) -> std::result::Result<ExecutionPlan, String> {
        self.generate_plan_with_policy(
            instruction,
            session_id,
            model_id,
            allowed_tools,
            tools_enabled,
            None,
        )
        .await
        .map_err(|error| error.to_string())
    }
}

/// 组装计划所需的会话级输入。
pub struct PlanAssemblyContext<'a> {
    pub session_id: &'a str,
    /// 会话工具白名单；步骤白名单与它取交集。
    pub session_tools: Option<Vec<String>>,
    pub tools_enabled: bool,
}

/// 把拆解结果组装成执行计划：一个意图 = 一个顺序步骤。
///
/// - `instruction` / `model_id` / `max_iterations` / `system_prompt_appendix` 取自意图；
/// - 工具白名单 = `session_tools ∩ intent.allowed_tools`，**不放宽**；
/// - `path = deep` 的意图先经 planner 展开，失败时退化为单个步骤。
pub async fn assemble_plan(
    decomposition: &Decomposition,
    ctx: &PlanAssemblyContext<'_>,
    expander: &dyn DeepExpander,
) -> ExecutionPlan {
    let mut steps: Vec<PlanStep> = Vec::new();

    for item in &decomposition.intents {
        let policy = IntentPolicy::from(item);
        // 路由器已解析出本回合作用域时以它为准（已含会话、意图与收窄建议）；
        // 未解析时沿用旧的白名单交集，行为不变。
        let allowed_tools = match decomposition.resolved.as_ref() {
            // 路由器已解析出本回合作用域（会话 ∩ 意图 ∩ 收窄建议），直接作为基线。
            Some(resolved) => Some(resolved.tools.clone()),
            None => intersect_tools(
                ctx.session_tools.clone(),
                policy.intent.config.enabled_tools.as_ref(),
            ),
        };

        if policy.intent.path == PathSelection::Deep {
            match expander
                .expand(
                    &policy.instruction,
                    ctx.session_id,
                    policy.intent.config.model_id.as_deref(),
                    allowed_tools.as_deref(),
                    ctx.tools_enabled,
                )
                .await
            {
                Ok(sub_plan) if !sub_plan.steps.is_empty() => {
                    steps.extend(deep_sub_steps(&policy, sub_plan, allowed_tools.clone()));
                }
                Ok(_) => {
                    log::warn!(
                        "deep intent '{}' expanded to an empty plan, falling back to a single step",
                        policy.intent.id
                    );
                    steps.push(single_step(&policy, allowed_tools));
                }
                Err(error) => {
                    log::warn!(
                        "deep intent '{}' failed to expand, falling back to a single step: {}",
                        policy.intent.id,
                        error
                    );
                    steps.push(single_step(&policy, allowed_tools));
                }
            }
        } else {
            steps.push(single_step(&policy, allowed_tools));
        }
    }

    let goal = steps
        .first()
        .map(|step| step.label.clone())
        .unwrap_or_default();

    ExecutionPlan {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: ctx.session_id.to_string(),
        source: PlanSource::Dynamic {
            goal,
            generated_by: "intent_decomposition".to_string(),
        },
        steps,
        status: PlanStatus::Pending,
        created_at: chrono::Utc::now().to_rfc3339(),
        finished_at: None,
    }
}

/// 展开出的子步骤：保留 planner 给的执行体，但把意图的配方叠加回去。
///
/// 工具白名单只会更窄：子步骤声明的工具必须落在意图白名单内，否则该步骤被
/// 丢弃（绝不放宽）。模型优先级为 intent > planner > registry default。
fn deep_sub_steps(
    policy: &IntentPolicy,
    sub_plan: ExecutionPlan,
    intent_tools: Option<Vec<String>>,
) -> Vec<PlanStep> {
    let mut out = Vec::new();

    for (index, step) in sub_plan.steps.into_iter().enumerate() {
        let crate::agent::orchestrator::plan_types::PlanStep { label, execution, .. } = step;

        // 意图白名单是硬约束：越界的工具调用步骤直接丢弃。
        if let ExecStep::ToolCall { tool, .. } = &execution {
            if let Some(intent) = intent_tools.as_ref() {
                if !intent.iter().any(|allowed| allowed == tool) {
                    log::warn!(
                        "dropping deep sub-step '{}' because tool '{}' is outside the intent whitelist",
                        label,
                        tool
                    );
                    continue;
                }
            }
        }

        // Pipeline 内部不受步骤白名单约束，无法安全收窄，直接丢弃。
        if matches!(execution, ExecStep::Pipeline { .. }) {
            log::warn!(
                "dropping deep sub-step '{}': pipeline steps cannot be constrained",
                label
            );
            continue;
        }

        let execution = match execution {
            ExecStep::AgentTask {
                instruction,
                model_id,
                max_iterations,
                allowed_tools,
                temperature,
                system_prompt_appendix,
            } => {
                let narrowed = match (intent_tools.as_ref(), allowed_tools.as_ref()) {
                    (Some(intent), Some(step_tools)) => {
                        intersect_tools(Some(intent.clone()), Some(step_tools))
                    }
                    (Some(intent), None) => Some(intent.clone()),
                    (None, _) => allowed_tools,
                };
                let model_id = policy.intent.config.model_id.clone().or(model_id);
                let system_prompt_appendix = policy
                    .intent
                    .config
                    .system_prompt_appendix
                    .clone()
                    .or(system_prompt_appendix);
                ExecStep::AgentTask {
                    instruction,
                    model_id,
                    max_iterations,
                    allowed_tools: narrowed,
                    temperature,
                    system_prompt_appendix,
                }
            }
            other => other,
        };

        out.push(PlanStep {
            id: format!("{}-{}", policy.intent.id, index + 1),
            label,
            execution,
            status: StepStatus::Pending,
            result: None,
            error: None,
            started_at: None,
            duration_ms: None,
            intent_id: Some(policy.intent.id.clone()),
            confidence: Some(policy.confidence),
            uncertain: policy.uncertain,
        });
    }

    out
}

/// 单个意图 → 单个 `AgentTask` 步骤。
fn single_step(policy: &IntentPolicy, allowed_tools: Option<Vec<String>>) -> PlanStep {
    PlanStep {
        id: uuid::Uuid::new_v4().to_string(),
        label: policy.intent.name.clone(),
        execution: ExecStep::AgentTask {
            instruction: policy.instruction.clone(),
            model_id: policy.intent.config.model_id.clone(),
            max_iterations: policy.intent.config.max_iterations,
            allowed_tools,
            temperature: None,
            system_prompt_appendix: policy.intent.config.system_prompt_appendix.clone(),
        },
        status: StepStatus::Pending,
        result: None,
        error: None,
        started_at: None,
        duration_ms: None,
        intent_id: Some(policy.intent.id.clone()),
        confidence: Some(policy.confidence),
        uncertain: policy.uncertain,
    }
}

// ── 持久化辅助 ──

/// 从已持久化的计划重建拆解结果（不调用模型）。
///
/// 每个带 `intent_id` 的步骤折算成一个意图项；同一意图只保留第一次出现
/// （deep 意图展开出的子步骤会重复同一 id）。
///
/// 返回值刻意用 `Option` 区分两种「空」：
/// - `None`：计划不可用（不存在 / 不属于该会话 / JSON 不可解析 / 有步骤但
///   映射不出任何意图），调用方应当重新拆解；
/// - `Some(d)`：计划可用，`d` 可能为空——0 步计划是合法的零意图会话，
///   调用方应当直接复用，不得再发起拆解。
pub(crate) fn decomposition_from_plan(
    db: &Database,
    session_id: &str,
    plan_id: &str,
) -> Option<Decomposition> {
    let record = match db.get_execution_plan(plan_id) {
        Ok(Some(record)) if record.session_id == session_id => record,
        Ok(_) => return None,
        Err(error) => {
            log::warn!("failed to read cached plan '{}': {error}", plan_id);
            return None;
        }
    };
    let plan: ExecutionPlan = match serde_json::from_str(&record.plan_json) {
        Ok(plan) => plan,
        Err(error) => {
            log::warn!("cached plan '{}' is not parseable: {error}", plan_id);
            return None;
        }
    };

    // 0 步计划是合法的零意图会话；有步骤却映射不出意图说明数据异常。
    if plan.steps.is_empty() {
        return Some(zero_decomposition());
    }

    let mut intents: Vec<DecomposedIntent> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for step in &plan.steps {
        let Some(intent_id) = step.intent_id.as_deref() else {
            continue;
        };
        if seen.iter().any(|id| id == intent_id) {
            continue;
        }
        let ExecStep::AgentTask { instruction, .. } = &step.execution else {
            continue;
        };
        seen.push(intent_id.to_string());
        intents.push(DecomposedIntent::new(
            projected_intent(db, intent_id, step),
            instruction.clone(),
            step.confidence.unwrap_or(0.0),
            step.uncertain,
        ));
    }

    if intents.is_empty() {
        log::warn!(
            "cached plan '{}' has steps but none map to an intent",
            plan_id
        );
        return None;
    }

    Some(Decomposition::from_intents(
        intents,
        DecisionSource::Llm,
        Vec::new(),
        DecisionEvidence::default(),
    ))
}

/// 优先用意图库里的现行记录投影；意图已被删除时退回计划步骤上的快照字段。
fn projected_intent(db: &Database, intent_id: &str, step: &PlanStep) -> IntentResult {
    if let Ok(Some(record)) = db.get_intent(intent_id) {
        return IntentRouter::project(&record, "", step.confidence.unwrap_or(0.0));
    }

    let (allowed_tools, model_id, max_iterations, appendix) = match &step.execution {
        ExecStep::AgentTask {
            allowed_tools,
            model_id,
            max_iterations,
            system_prompt_appendix,
            ..
        } => (
            allowed_tools.clone(),
            model_id.clone(),
            *max_iterations,
            system_prompt_appendix.clone(),
        ),
        _ => (None, None, None, None),
    };

    IntentResult {
        id: intent_id.to_string(),
        name: step.label.clone(),
        description: String::new(),
        domain_id: String::new(),
        domain_name: String::new(),
        config: crate::llm::intent::IntentConfig {
            system_prompt_appendix: appendix,
            enabled_tools: allowed_tools,
            model_id,
            max_iterations,
            auto_escalate: false,
        },
        path: PathSelection::Fast,
        confidence: step.confidence.unwrap_or(0.0),
    }
}

/// 把计划序列化成数据库记录。
pub(crate) fn plan_record(plan: &ExecutionPlan, status: &str) -> ExecutionPlanRecord {
    ExecutionPlanRecord {
        id: plan.id.clone(),
        session_id: plan.session_id.clone(),
        source: serde_json::to_string(&plan.source).unwrap_or_default(),
        goal: match &plan.source {
            PlanSource::Dynamic { goal, .. } => Some(goal.clone()),
            PlanSource::Static { .. } => None,
        },
        plan_json: serde_json::to_string(plan).unwrap_or_default(),
        status: status.to_string(),
        created_at: plan.created_at.clone(),
        finished_at: plan.finished_at.clone(),
    }
}

/// 把计划步骤展开成数据库记录，`step_index` 按当前顺序连续编号。
pub(crate) fn plan_step_records(plan: &ExecutionPlan) -> Vec<PlanStepRecord> {
    plan.steps
        .iter()
        .enumerate()
        .map(|(index, step)| PlanStepRecord {
            id: step.id.clone(),
            plan_id: plan.id.clone(),
            step_index: index as i32,
            label: step.label.clone(),
            step_type: exec_step_type_name(&step.execution).to_string(),
            status: step.status.as_str().to_string(),
            result_json: step.result.as_ref().map(|value| value.to_string()),
            error: step.error.clone(),
            started_at: step.started_at.clone(),
            duration_ms: step.duration_ms.map(|value| value as i64),
        })
        .collect()
}

/// 落一条 `pending` 计划 + 对应步骤。**不执行任何步骤**。
pub(crate) fn persist_pending_plan(db: &Database, plan: &ExecutionPlan) -> Result<()> {
    let record = plan_record(plan, PlanStatus::Pending.as_str());
    db.upsert_execution_plan(&record)?;
    db.replace_plan_steps(&plan.id, &plan_step_records(plan))?;
    Ok(())
}

/// 把一次拆解结果落成会话缓存计划，并把会话指向它。
///
/// - **合法零意图**（模型明确返回空数组）→ 落一条 0 步计划。它是缓存标记，
///   让后续消息直接复用、不再重复发起拆解调用（每条消息原本会烧 2 次 LLM）。
/// - **不可用**（超时 / 调用失败）→ `Ok(None)`：不落计划、不动
///   `active_plan_id`，后续消息可以重试；一次的瞬时失败不能被固化成
///   「这个会话永远零意图」。
///
/// 计划组装可能在 deep 意图上调用模型，因此先组装、后加锁落库。
pub(crate) async fn cache_decomposition_plan(
    db: &std::sync::Arc<tokio::sync::Mutex<Database>>,
    session_id: &str,
    decomposition: &Decomposition,
    session_tools: Option<Vec<String>>,
    expander: &dyn DeepExpander,
) -> Result<Option<ExecutionPlan>> {
    if decomposition.source == DecisionSource::Unavailable {
        return Ok(None);
    }

    let plan = assemble_plan(
        decomposition,
        &PlanAssemblyContext {
            session_id,
            session_tools,
            tools_enabled: true,
        },
        expander,
    )
    .await;

    {
        let db = db.lock().await;
        persist_pending_plan(&db, &plan)?;
        db.set_session_active_plan(session_id, Some(&plan.id))?;
    }

    Ok(Some(plan))
}

/// 读回一个仍处于 `pending`（未确认）的计划；其它状态一律拒绝执行。
pub(crate) fn load_pending_plan(db: &Database, plan_id: &str) -> Result<ExecutionPlan> {
    let record = db
        .get_execution_plan(plan_id)?
        .ok_or_else(|| AppError::NotFound(format!("计划不存在: {plan_id}")))?;
    if record.status != PlanStatus::Pending.as_str() {
        return Err(AppError::InvalidInput(format!(
            "计划当前状态为「{}」，只有待确认的计划可编辑或执行",
            record.status
        )));
    }
    let plan: ExecutionPlan = serde_json::from_str(&record.plan_json)?;
    Ok(plan)
}

/// 校验一个计划可以被该会话执行。
pub(crate) fn load_confirmable_plan(
    db: &Database,
    session_id: &str,
    plan_id: &str,
) -> Result<ExecutionPlan> {
    let plan = load_pending_plan(db, plan_id)?;
    if plan.session_id != session_id {
        return Err(AppError::InvalidInput("该计划不属于当前会话".to_string()));
    }
    if plan.steps.is_empty() {
        return Err(AppError::InvalidInput("计划没有可执行步骤".to_string()));
    }
    Ok(plan)
}

/// 改写一个待确认步骤的指令。
pub(crate) fn apply_step_instruction(
    plan: &mut ExecutionPlan,
    step_id: &str,
    instruction: &str,
) -> Result<()> {
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Err(AppError::InvalidInput("步骤指令不能为空".to_string()));
    }
    let step = plan
        .steps
        .iter_mut()
        .find(|step| step.id == step_id)
        .ok_or_else(|| AppError::NotFound(format!("步骤不存在: {step_id}")))?;
    match &mut step.execution {
        ExecStep::AgentTask {
            instruction: current,
            ..
        } => {
            *current = instruction.to_string();
            Ok(())
        }
        _ => Err(AppError::InvalidInput(
            "只有 agent 任务步骤可以改写指令".to_string(),
        )),
    }
}

/// 从待确认计划里移除一个步骤；`step_index` 由 [`plan_step_records`] 重排。
pub(crate) fn remove_step(plan: &mut ExecutionPlan, step_id: &str) -> Result<()> {
    let before = plan.steps.len();
    plan.steps.retain(|step| step.id != step_id);
    if plan.steps.len() == before {
        return Err(AppError::NotFound(format!("步骤不存在: {step_id}")));
    }
    if plan.steps.is_empty() {
        return Err(AppError::InvalidInput(
            "不能移除计划的最后一个步骤".to_string(),
        ));
    }
    Ok(())
}

/// 写回编辑后的计划（确认卡操作）。
pub(crate) fn persist_edited_plan(db: &Database, plan: &ExecutionPlan) -> Result<()> {
    let plan_json = serde_json::to_string(plan)?;
    db.update_execution_plan_json(&plan.id, &plan_json)?;
    db.replace_plan_steps(&plan.id, &plan_step_records(plan))?;
    Ok(())
}

/// 丢弃一个待确认计划：标记 `cancelled` 并解除会话指向；历史记录保留。
pub(crate) fn cancel_pending_plan(db: &Database, plan_id: &str) -> Result<()> {
    let record = db
        .get_execution_plan(plan_id)?
        .ok_or_else(|| AppError::NotFound(format!("计划不存在: {plan_id}")))?;
    if record.status == PlanStatus::Cancelled.as_str() {
        return Ok(());
    }
    if record.status != PlanStatus::Pending.as_str() {
        return Err(AppError::InvalidInput(format!(
            "计划当前状态为「{}」，不能取消",
            record.status
        )));
    }

    let finished_at = chrono::Utc::now().to_rfc3339();
    db.update_execution_plan_status(
        plan_id,
        PlanStatus::Cancelled.as_str(),
        Some(&finished_at),
    )?;

    if let Some(session) = db.get_session(&record.session_id)? {
        if session.active_plan_id.as_deref() == Some(plan_id) {
            db.clear_session_active_plan(&record.session_id)?;
        }
    }
    Ok(())
}

// ── IPC ──

/// 拆解结果里的单个意图，供确认卡展示与执行配方回显。
#[derive(Debug, Clone, Serialize)]
pub struct DecomposedIntentPayload {
    pub intent_id: String,
    pub intent_name: String,
    pub instruction: String,
    pub confidence: f32,
    pub uncertain: bool,
    pub allowed_tools: Vec<String>,
    pub model_id: Option<String>,
    pub max_iterations: Option<usize>,
    pub path: String,
    pub probe_evidence: Option<String>,
}

/// 本回合资源作用域的可展示快照（只读，供确认卡说明「这次会用哪些资源」）。
#[derive(Debug, Clone, Serialize)]
pub struct ScopePayload {
    /// 稳定英文标签：`not_requested` / `skipped_*` / `applied`。
    pub status: String,
    /// 第二级收窄是否命中缓存（仅 `applied` 时有意义）。
    pub cached: bool,
    /// 最终允许的工具 id（已按 `(source, id)` 排序）。
    pub tools: Vec<String>,
    /// 最终注入的提示词片段 id。
    pub prompts: Vec<String>,
    /// 记忆条数上限；`None` 表示沿用默认。
    pub memory_limit: Option<usize>,
    /// 是否因预算被截断。
    pub truncated: bool,
}

/// `decompose_message` 的返回值。
#[derive(Debug, Clone, Serialize)]
pub struct DecomposeMessageResult {
    /// 零意图时为空：调用方应走普通聊天，不发送计划。
    ///
    /// 注意：合法零意图也会落一条 0 步缓存计划，此时 `plan_id` 仍是 `Some`，
    /// 但 `intents` 为空——前端不应因此弹出确认卡，只同步会话缓存指向。
    pub plan_id: Option<String>,
    pub intents: Vec<DecomposedIntentPayload>,
    pub confidence: f32,
    pub uncertain: bool,
    /// 分解器不可用（超时 / 调用失败）：本条消息按普通聊天处理，且未落缓存，
    /// 前端可以提示用户，并在下一条消息重试。
    #[serde(default)]
    pub unavailable: bool,
    /// 本回合资源作用域；`resolved` 为 `None` 时工具/提示词为空，`status` 说明原因。
    pub scope: ScopePayload,
}

fn intent_payload(
    session_tools: Option<&[String]>,
    item: &DecomposedIntent,
) -> DecomposedIntentPayload {
    let allowed_tools = intersect_tools(
        session_tools.map(|tools| tools.to_vec()),
        item.intent.config.enabled_tools.as_ref(),
    )
    .unwrap_or_default();

    DecomposedIntentPayload {
        intent_id: item.intent.id.clone(),
        intent_name: item.intent.name.clone(),
        instruction: item.instruction.clone(),
        confidence: item.confidence,
        uncertain: item.uncertain,
        allowed_tools,
        model_id: item.intent.config.model_id.clone(),
        max_iterations: item.intent.config.max_iterations,
        path: match item.intent.path {
            PathSelection::Deep => "deep".to_string(),
            PathSelection::Fast => "fast".to_string(),
        },
        probe_evidence: item.probe_evidence.clone(),
    }
}

/// 把拆解结果的作用域投影成只读快照；未解析时只给出状态标签。
pub(crate) fn scope_payload(decomposition: &Decomposition) -> ScopePayload {
    let cached = match decomposition.scope_status {
        ScopeStatus::Applied { cached, .. } => cached,
        _ => false,
    };
    let resolved = decomposition.resolved.as_ref();
    ScopePayload {
        status: decomposition.scope_status.label().to_string(),
        cached,
        tools: resolved.map(|r| r.tools.clone()).unwrap_or_default(),
        prompts: resolved.map(|r| r.prompts.clone()).unwrap_or_default(),
        memory_limit: resolved.and_then(|r| r.memory_limit),
        truncated: resolved.map(|r| r.truncated).unwrap_or(false),
    }
}

/// 拆解一条消息并落一条待确认计划。
///
/// 合法零意图也会落一条 0 步计划并挂到会话上作为拆解缓存；只有分解器
/// 不可用（超时 / 调用失败）才完全不落库，且结果标记 `unavailable`。
#[tauri::command]
pub async fn decompose_message(
    state: State<'_, AppState>,
    session_id: String,
    content: String,
) -> Result<DecomposeMessageResult> {
    let (session, messages) = {
        let db = state.db.lock().await;
        let session = db
            .get_session(&session_id)?
            .ok_or_else(|| AppError::NotFound(format!("会话不存在: {session_id}")))?;
        let messages = db.get_messages(&session_id)?;
        (session, messages)
    };

    // 拆解发生在「发送之前」：本轮用户消息**不落库**（取消 = 消息没发出去）。
    let decomposition = crate::app::commands::session::resolve_decomposition(
        state.intent_router.as_ref(),
        &state.db,
        &session_id,
        &session,
        &messages,
        None,
        &content,
        None,
    )
    .await?
    .decomposition;
    emit_intent_resolved(state.inner(), &session_id, &decomposition);

    let session_tools = session_enabled_tools(&session);
    let intents: Vec<DecomposedIntentPayload> = decomposition
        .intents
        .iter()
        .map(|item| intent_payload(session_tools.as_deref(), item))
        .collect();

    let planner = LlmPlanner::new(state.providers.clone(), state.tools.clone());
    let cached = cache_decomposition_plan(
        &state.db,
        &session_id,
        &decomposition,
        session_tools,
        &planner,
    )
    .await?;

    Ok(DecomposeMessageResult {
        plan_id: cached.as_ref().map(|plan| plan.id.clone()),
        intents,
        confidence: decomposition.confidence,
        uncertain: decomposition.uncertain,
        unavailable: decomposition.source == DecisionSource::Unavailable,
        scope: scope_payload(&decomposition),
    })
}

/// 改写某个待确认步骤的指令。
#[tauri::command]
pub async fn update_plan_step(
    state: State<'_, AppState>,
    plan_id: String,
    step_id: String,
    instruction: String,
) -> Result<ExecutionPlan> {
    let db = state.db.lock().await;
    let mut plan = load_pending_plan(&db, &plan_id)?;
    apply_step_instruction(&mut plan, &step_id, &instruction)?;
    persist_edited_plan(&db, &plan)?;
    Ok(plan)
}

/// 移除某个待确认步骤，并重排其余步骤的 `step_index`。
#[tauri::command]
pub async fn remove_plan_step(
    state: State<'_, AppState>,
    plan_id: String,
    step_id: String,
) -> Result<ExecutionPlan> {
    let db = state.db.lock().await;
    let mut plan = load_pending_plan(&db, &plan_id)?;
    remove_step(&mut plan, &step_id)?;
    persist_edited_plan(&db, &plan)?;
    Ok(plan)
}

/// 丢弃一个待确认计划（取消 = 该条消息不发送）。
#[tauri::command]
pub async fn cancel_plan(state: State<'_, AppState>, plan_id: String) -> Result<()> {
    let db = state.db.lock().await;
    cancel_pending_plan(&db, &plan_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::Session;
    use crate::llm::intent::{DecomposedIntent, IntentConfig, IntentResult};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn intent(id: &str, path: PathSelection) -> IntentResult {
        IntentResult {
            id: id.to_string(),
            name: format!("意图 {id}"),
            description: format!("{id} 的说明"),
            domain_id: "test.domain".to_string(),
            domain_name: "程序开发".to_string(),
            config: IntentConfig {
                system_prompt_appendix: Some("be strict".to_string()),
                enabled_tools: Some(vec!["read".to_string(), "shell".to_string()]),
                model_id: Some("model-a".to_string()),
                max_iterations: Some(5),
                auto_escalate: false,
            },
            path,
            confidence: 0.8,
        }
    }

    fn decomposition(items: Vec<DecomposedIntent>) -> Decomposition {
        Decomposition::from_intents(items, DecisionSource::Llm, Vec::new(), DecisionEvidence::default())
    }

    fn ctx<'a>(session_tools: Option<Vec<String>>) -> PlanAssemblyContext<'a> {
        PlanAssemblyContext {
            session_id: "session-1",
            session_tools,
            tools_enabled: true,
        }
    }

    struct FixedExpander {
        steps: Vec<PlanStep>,
        fail: bool,
        calls: AtomicUsize,
    }

    impl FixedExpander {
        fn ok(steps: Vec<PlanStep>) -> Self {
            Self {
                steps,
                fail: false,
                calls: AtomicUsize::new(0),
            }
        }

        fn failing() -> Self {
            Self {
                steps: Vec::new(),
                fail: true,
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl DeepExpander for FixedExpander {
        async fn expand(
            &self,
            _instruction: &str,
            _session_id: &str,
            _model_id: Option<&str>,
            _allowed_tools: Option<&[String]>,
            _tools_enabled: bool,
        ) -> std::result::Result<ExecutionPlan, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("planner unavailable".to_string());
            }
            Ok(ExecutionPlan {
                id: "sub".to_string(),
                session_id: "session-1".to_string(),
                source: PlanSource::Dynamic {
                    goal: "sub".to_string(),
                    generated_by: "test".to_string(),
                },
                steps: self.steps.clone(),
                status: PlanStatus::Pending,
                created_at: "2026-11-09T00:00:00Z".to_string(),
                finished_at: None,
            })
        }
    }

    fn sub_step(id: &str, execution: ExecStep) -> PlanStep {
        PlanStep {
            id: id.to_string(),
            label: format!("Sub {id}"),
            execution,
            status: StepStatus::Pending,
            result: None,
            error: None,
            started_at: None,
            duration_ms: None,
            intent_id: None,
            confidence: None,
            uncertain: false,
        }
    }

    fn sub_agent(instruction: &str, allowed_tools: Option<Vec<String>>) -> ExecStep {
        ExecStep::AgentTask {
            instruction: instruction.to_string(),
            model_id: Some("planner-model".to_string()),
            max_iterations: Some(3),
            allowed_tools,
            temperature: None,
            system_prompt_appendix: None,
        }
    }

    #[tokio::test]
    async fn intent_maps_to_an_agent_task_with_its_recipe() {
        let decomposition = decomposition(vec![DecomposedIntent::new(
            intent("test.a", PathSelection::Fast),
            "实现 CSV 导出",
            0.8,
            true,
        )]);
        let expander = FixedExpander::failing();

        let plan = assemble_plan(
            &decomposition,
            &ctx(Some(vec![
                "read".to_string(),
                "shell".to_string(),
                "write".to_string(),
            ])),
            &expander,
        )
        .await;

        assert_eq!(plan.steps.len(), 1);
        let step = &plan.steps[0];
        assert_eq!(step.label, "意图 test.a");
        assert_eq!(step.intent_id.as_deref(), Some("test.a"));
        assert_eq!(step.confidence, Some(0.8));
        assert!(step.uncertain);
        match &step.execution {
            ExecStep::AgentTask {
                instruction,
                model_id,
                max_iterations,
                allowed_tools,
                system_prompt_appendix,
                ..
            } => {
                assert_eq!(instruction, "实现 CSV 导出");
                assert_eq!(model_id.as_deref(), Some("model-a"));
                assert_eq!(*max_iterations, Some(5));
                assert_eq!(
                    allowed_tools.clone().unwrap_or_default(),
                    vec!["read".to_string(), "shell".to_string()]
                );
                assert_eq!(system_prompt_appendix.as_deref(), Some("be strict"));
            }
            other => panic!("expected an agent task, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn step_tools_are_the_intersection_and_never_widened() {
        let decomposition = decomposition(vec![DecomposedIntent::new(
            intent("test.a", PathSelection::Fast),
            "只读任务",
            0.9,
            false,
        )]);
        let expander = FixedExpander::failing();

        let plan = assemble_plan(
            &decomposition,
            &ctx(Some(vec!["shell".to_string(), "read".to_string()])),
            &expander,
        )
        .await;

        let ExecStep::AgentTask { allowed_tools, .. } = &plan.steps[0].execution else {
            panic!("expected an agent task");
        };
        assert_eq!(
            allowed_tools.clone().unwrap_or_default(),
            vec!["shell".to_string(), "read".to_string()]
        );

        // 会话没有 shell 时，意图也不能凭空引入它。
        let plan = assemble_plan(
            &decomposition,
            &ctx(Some(vec!["read".to_string()])),
            &expander,
        )
        .await;
        let ExecStep::AgentTask { allowed_tools, .. } = &plan.steps[0].execution else {
            panic!("expected an agent task");
        };
        assert_eq!(
            allowed_tools.clone().unwrap_or_default(),
            vec!["read".to_string()]
        );
    }

    #[tokio::test]
    async fn uncertainty_does_not_widen_the_step_tools() {
        let certain = decomposition(vec![DecomposedIntent::new(
            intent("test.a", PathSelection::Fast),
            "do it",
            0.9,
            false,
        )]);
        let uncertain = decomposition(vec![DecomposedIntent::new(
            intent("test.a", PathSelection::Fast),
            "do it",
            0.1,
            true,
        )]);
        let expander = FixedExpander::failing();
        let session_tools = Some(vec!["read".to_string()]);

        let certain_plan = assemble_plan(&certain, &ctx(session_tools.clone()), &expander).await;
        let uncertain_plan = assemble_plan(&uncertain, &ctx(session_tools), &expander).await;

        let ExecStep::AgentTask {
            allowed_tools: a, ..
        } = &certain_plan.steps[0].execution
        else {
            panic!("expected an agent task");
        };
        let ExecStep::AgentTask {
            allowed_tools: b, ..
        } = &uncertain_plan.steps[0].execution
        else {
            panic!("expected an agent task");
        };
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn deep_intent_expands_into_sub_steps_with_narrowed_tools() {
        let mut deep = intent("test.deep", PathSelection::Deep);
        deep.config.enabled_tools = Some(vec!["read".to_string()]);
        let decomposition = decomposition(vec![DecomposedIntent::new(deep, "重构模块", 0.7, false)]);

        let expander = FixedExpander::ok(vec![
            sub_step("1", sub_agent("先读文件", Some(vec!["read".to_string(), "write".to_string()]))),
            sub_step(
                "2",
                ExecStep::ToolCall {
                    tool: "shell".to_string(),
                    params: Default::default(),
                    retry: None,
                    timeout_seconds: None,
                },
            ),
            sub_step("3", sub_agent("再改代码", None)),
        ]);

        let plan = assemble_plan(
            &decomposition,
            &ctx(Some(vec!["read".to_string(), "shell".to_string()])),
            &expander,
        )
        .await;

        assert_eq!(expander.calls(), 1);
        // 越界的 shell 工具步骤被丢弃，剩下两个 agent 步骤。
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].intent_id.as_deref(), Some("test.deep"));
        assert_eq!(plan.steps[0].id, "test.deep-1");
        match &plan.steps[0].execution {
            ExecStep::AgentTask {
                instruction,
                model_id,
                allowed_tools,
                system_prompt_appendix,
                ..
            } => {
                assert_eq!(instruction, "先读文件");
                assert_eq!(model_id.as_deref(), Some("model-a"), "意图模型优先");
                assert_eq!(
                    allowed_tools.clone().unwrap_or_default(),
                    vec!["read".to_string()]
                );
                assert_eq!(system_prompt_appendix.as_deref(), Some("be strict"));
            }
            other => panic!("expected an agent task, got {other:?}"),
        }
        match &plan.steps[1].execution {
            ExecStep::AgentTask { allowed_tools, .. } => assert_eq!(
                allowed_tools.clone().unwrap_or_default(),
                vec!["read".to_string()]
            ),
            other => panic!("expected an agent task, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn deep_expansion_failure_falls_back_to_a_single_step() {
        let decomposition = decomposition(vec![DecomposedIntent::new(
            intent("test.deep", PathSelection::Deep),
            "重构模块",
            0.7,
            false,
        )]);
        let expander = FixedExpander::failing();

        let plan = assemble_plan(&decomposition, &ctx(None), &expander).await;

        assert_eq!(plan.steps.len(), 1);
        let ExecStep::AgentTask { instruction, .. } = &plan.steps[0].execution else {
            panic!("expected an agent task");
        };
        assert_eq!(instruction, "重构模块");
    }

    // ── 计划持久化与确认 ──

    fn test_db() -> Database {
        Database::new_test().expect("in-memory db")
    }

    fn insert_session(db: &Database, id: &str) {
        let now = "2026-11-09T00:00:00Z".to_string();
        db.create_session(&Session {
            id: id.to_string(),
            title: id.to_string(),
            model_id: "test-model".to_string(),
            system_prompt: None,
            persona_id: None,
            config: None,
            title_source: "manual".to_string(),
            archived: false,
            created_at: now.clone(),
            updated_at: now,
            mode: "chat".to_string(),
            execution_status: String::new(),
            active_plan_id: None,
            project_id: None,
            intent_domain_id: None,
            intent_domain_mode: "none".to_string(),
        })
        .expect("create session");
    }

    async fn sample_plan(session_id: &str, count: usize) -> ExecutionPlan {
        let items: Vec<DecomposedIntent> = (0..count)
            .map(|index| {
                DecomposedIntent::new(
                    intent(&format!("test.{index}"), PathSelection::Fast),
                    format!("do {index}"),
                    0.9,
                    false,
                )
            })
            .collect();
        assemble_plan(
            &decomposition(items),
            &PlanAssemblyContext {
                session_id,
                session_tools: None,
                tools_enabled: true,
            },
            &FixedExpander::failing(),
        )
        .await
    }

    #[tokio::test]
    async fn pending_plan_has_no_side_effects_until_confirmed() {
        let db = test_db();
        insert_session(&db, "session-1");
        let plan = sample_plan("session-1", 2).await;

        persist_pending_plan(&db, &plan).expect("persist pending plan");

        let record = db.get_execution_plan(&plan.id).unwrap().expect("plan row");
        assert_eq!(record.status, PlanStatus::Pending.as_str());
        let steps = db.get_plan_steps(&plan.id).unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps.iter().all(|step| step.status == "pending"));
        assert!(steps.iter().all(|step| step.result_json.is_none()));
        assert!(
            db.get_messages("session-1").unwrap().is_empty(),
            "确认前不得产生任何消息（也就没有步骤副作用）"
        );

        // 未确认的计划可以被执行入口读取；取消后则不行。
        assert!(load_confirmable_plan(&db, "session-1", &plan.id).is_ok());
        cancel_pending_plan(&db, &plan.id).expect("cancel");
        assert!(load_confirmable_plan(&db, "session-1", &plan.id).is_err());
    }

    #[tokio::test]
    async fn confirmable_plan_rejects_a_foreign_session() {
        let db = test_db();
        insert_session(&db, "session-1");
        insert_session(&db, "session-2");
        let plan = sample_plan("session-1", 1).await;
        persist_pending_plan(&db, &plan).unwrap();

        assert!(load_confirmable_plan(&db, "session-2", &plan.id).is_err());
    }

    #[tokio::test]
    async fn editing_a_step_rewrites_only_that_instruction() {
        let db = test_db();
        insert_session(&db, "session-1");
        let mut plan = sample_plan("session-1", 2).await;
        persist_pending_plan(&db, &plan).unwrap();
        let step_id = plan.steps[0].id.clone();

        apply_step_instruction(&mut plan, &step_id, "改写成新指令").expect("rewrite instruction");
        persist_edited_plan(&db, &plan).expect("persist edit");

        let reloaded = load_pending_plan(&db, &plan.id).unwrap();
        let ExecStep::AgentTask { instruction, .. } = &reloaded.steps[0].execution else {
            panic!("expected an agent task");
        };
        assert_eq!(instruction, "改写成新指令");
        let ExecStep::AgentTask {
            instruction: untouched,
            ..
        } = &reloaded.steps[1].execution
        else {
            panic!("expected an agent task");
        };
        assert_eq!(untouched, "do 1");
    }

    #[tokio::test]
    async fn removing_a_step_reindexes_the_rest() {
        let db = test_db();
        insert_session(&db, "session-1");
        let mut plan = sample_plan("session-1", 3).await;
        persist_pending_plan(&db, &plan).unwrap();
        let removed = plan.steps[1].id.clone();

        remove_step(&mut plan, &removed).expect("remove step");
        persist_edited_plan(&db, &plan).expect("persist edit");

        let steps = db.get_plan_steps(&plan.id).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].step_index, 0);
        assert_eq!(steps[1].step_index, 1);
        assert!(steps.iter().all(|step| step.id != removed));
    }

    #[tokio::test]
    async fn the_last_step_cannot_be_removed() {
        let mut plan = sample_plan("session-1", 1).await;
        let step_id = plan.steps[0].id.clone();

        assert!(remove_step(&mut plan, &step_id).is_err());
    }

    #[tokio::test]
    async fn cancelling_a_plan_keeps_history_and_clears_the_cache_pointer() {
        let db = test_db();
        insert_session(&db, "session-1");
        let plan = sample_plan("session-1", 1).await;
        persist_pending_plan(&db, &plan).unwrap();
        db.set_session_active_plan("session-1", Some(&plan.id))
            .unwrap();

        cancel_pending_plan(&db, &plan.id).unwrap();

        let record = db.get_execution_plan(&plan.id).unwrap().expect("history kept");
        assert_eq!(record.status, PlanStatus::Cancelled.as_str());
        let session = db.get_session("session-1").unwrap().expect("session");
        assert!(session.active_plan_id.is_none());
    }

    #[tokio::test]
    async fn cached_plan_rebuilds_the_intent_list_without_a_model_call() {
        let db = test_db();
        insert_session(&db, "session-1");
        let plan = sample_plan("session-1", 2).await;
        persist_pending_plan(&db, &plan).unwrap();

        let rebuilt = decomposition_from_plan(&db, "session-1", &plan.id).expect("plan available");

        assert_eq!(rebuilt.intents.len(), 2);
        assert_eq!(rebuilt.intents[0].instruction, "do 0");
        assert_eq!(rebuilt.intents[1].instruction, "do 1");
        assert_eq!(rebuilt.intents[0].intent.name, "意图 test.0");
    }

    #[tokio::test]
    async fn a_missing_or_foreign_plan_is_unavailable() {
        let db = test_db();
        insert_session(&db, "session-1");
        insert_session(&db, "session-2");
        let plan = sample_plan("session-1", 1).await;
        persist_pending_plan(&db, &plan).unwrap();

        assert!(decomposition_from_plan(&db, "session-1", "no-such-plan").is_none());
        assert!(
            decomposition_from_plan(&db, "session-2", &plan.id).is_none(),
            "别人的计划不算本会话的缓存"
        );
    }

    #[tokio::test]
    async fn a_zero_step_plan_rebuilds_to_an_available_empty_decomposition() {
        let db = test_db();
        insert_session(&db, "session-1");
        let mut plan = sample_plan("session-1", 1).await;
        plan.steps.clear();
        persist_pending_plan(&db, &plan).unwrap();

        let rebuilt = decomposition_from_plan(&db, "session-1", &plan.id)
            .expect("0 步计划仍是可用的缓存");

        assert!(rebuilt.is_empty());
    }

    #[tokio::test]
    async fn intent_library_change_invalidates_every_session_cache() {
        let db = test_db();
        insert_session(&db, "session-1");
        insert_session(&db, "session-2");
        let first = sample_plan("session-1", 1).await;
        let second = sample_plan("session-2", 1).await;
        persist_pending_plan(&db, &first).unwrap();
        persist_pending_plan(&db, &second).unwrap();
        db.set_session_active_plan("session-1", Some(&first.id))
            .unwrap();
        db.set_session_active_plan("session-2", Some(&second.id))
            .unwrap();

        db.clear_all_active_plans().unwrap();

        for id in ["session-1", "session-2"] {
            let session = db.get_session(id).unwrap().expect("session");
            assert!(session.active_plan_id.is_none());
        }
        // 计划历史记录保留。
        assert!(db.get_execution_plan(&first.id).unwrap().is_some());
        assert!(db.get_execution_plan(&second.id).unwrap().is_some());
    }

    #[tokio::test]
    async fn switching_domain_invalidates_only_that_session() {
        let db = test_db();
        insert_session(&db, "session-1");
        insert_session(&db, "session-2");
        let first = sample_plan("session-1", 1).await;
        let second = sample_plan("session-2", 1).await;
        persist_pending_plan(&db, &first).unwrap();
        persist_pending_plan(&db, &second).unwrap();
        db.set_session_active_plan("session-1", Some(&first.id))
            .unwrap();
        db.set_session_active_plan("session-2", Some(&second.id))
            .unwrap();

        db.clear_session_active_plan("session-1").unwrap();

        assert!(db
            .get_session("session-1")
            .unwrap()
            .expect("session")
            .active_plan_id
            .is_none());
        assert_eq!(
            db.get_session("session-2")
                .unwrap()
                .expect("session")
                .active_plan_id
                .as_deref(),
            Some(second.id.as_str())
        );
    }

    // ── 拆解缓存：合法零意图 vs 分解器不可用 ──

    #[tokio::test]
    async fn unavailable_decomposition_is_not_cached_as_a_plan() {
        let db = Arc::new(tokio::sync::Mutex::new(test_db()));
        {
            let guard = db.lock().await;
            insert_session(&guard, "session-1");
        }
        let unavailable = Decomposition::zero_intent(
            DecisionSource::Unavailable,
            Vec::new(),
            DecisionEvidence::default(),
        );

        let cached = cache_decomposition_plan(
            &db,
            "session-1",
            &unavailable,
            None,
            &FixedExpander::failing(),
        )
        .await
        .expect("cache attempt");

        assert!(cached.is_none(), "不可用不得落计划");
        let guard = db.lock().await;
        assert!(guard.list_execution_plans("session-1").unwrap().is_empty());
        assert!(
            guard
                .get_session("session-1")
                .unwrap()
                .expect("session")
                .active_plan_id
                .is_none(),
            "不可用时不得设 active_plan_id，否则瞬时失败会被永久固化"
        );
    }

    #[tokio::test]
    async fn legal_zero_intent_is_cached_as_a_zero_step_plan() {
        let db = Arc::new(tokio::sync::Mutex::new(test_db()));
        {
            let guard = db.lock().await;
            insert_session(&guard, "session-1");
        }
        let zero = Decomposition::zero_intent(
            DecisionSource::Fallback,
            Vec::new(),
            DecisionEvidence::default(),
        );

        let cached = cache_decomposition_plan(
            &db,
            "session-1",
            &zero,
            None,
            &FixedExpander::failing(),
        )
        .await
        .expect("cache zero intent")
        .expect("合法零意图必须落缓存");

        assert!(cached.steps.is_empty(), "零意图缓存是 0 步计划");
        let guard = db.lock().await;
        assert_eq!(
            guard
                .get_session("session-1")
                .unwrap()
                .expect("session")
                .active_plan_id
                .as_deref(),
            Some(cached.id.as_str()),
            "零意图缓存必须挂到会话上，后续消息才能复用"
        );
        let rebuilt = decomposition_from_plan(&guard, "session-1", &cached.id)
            .expect("0 步计划仍是可用的缓存");
        assert!(rebuilt.is_empty());
    }

    fn resolved_scope() -> crate::llm::intent::scope::ResolvedScope {
        crate::llm::intent::scope::ResolvedScope {
            tools: vec!["builtin:read".to_string(), "builtin:shell".to_string()],
            prompts: vec!["intent.dev".to_string()],
            memory_limit: Some(5),
            truncated: true,
            removals: Vec::new(),
        }
    }

    #[test]
    fn scope_payload_is_empty_when_not_resolved() {
        let mut decomposition = decomposition(Vec::new());
        decomposition.scope_status = ScopeStatus::SkippedSmallCatalog;

        let payload = scope_payload(&decomposition);

        assert_eq!(payload.status, "skipped_small_catalog");
        assert!(!payload.cached);
        assert!(payload.tools.is_empty());
        assert!(payload.prompts.is_empty());
        assert_eq!(payload.memory_limit, None);
        assert!(!payload.truncated);
    }

    #[test]
    fn scope_payload_mirrors_resolved_scope() {
        let mut decomposition = decomposition(Vec::new());
        decomposition.scope_status = ScopeStatus::Applied {
            selected: 2,
            catalog_size: 9,
            cached: true,
        };
        decomposition.resolved = Some(resolved_scope());

        let payload = scope_payload(&decomposition);

        assert_eq!(payload.status, "applied");
        assert!(payload.cached);
        assert_eq!(payload.tools, vec!["builtin:read", "builtin:shell"]);
        assert_eq!(payload.prompts, vec!["intent.dev"]);
        assert_eq!(payload.memory_limit, Some(5));
        assert!(payload.truncated);
    }
}
