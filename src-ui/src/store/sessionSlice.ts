import type { StateCreator } from 'zustand';
import type { Session, Message, SessionMode, ExecStatus, ExecutionPlan, PlanProgressEvent, ExecutionLogEntry, ToolInfo, SystemPrompt } from '../types';
import type { IntentResolvedEvent, SessionIntentState } from '../api/intent';
import type { DecomposedIntentPayload, PlanConfirmIntent, ScopePayload } from '../api/plan';
import type { SessionLogEvent, TurnLogEntry, TurnLogSummary } from '../api/session';
import { buildPlanConfirmIntents } from '../api/plan';
import * as api from '../api/tauri';

export interface SessionSlice {
  sessions: Session[];
  currentSession: Session | null;
  messages: Message[];

  // Execution state
  sessionMode: SessionMode;
  executionStatus: ExecStatus;
  activePlan: ExecutionPlan | null;
  planProgress: PlanProgressEvent | null;
  executionLogs: ExecutionLogEntry[];

  // Intent domain state for the active session
  intentDomainId: string | null;
  intentDomainName: string | null;
  intentDomainMode: 'pinned' | 'none';
  currentTurnIntent: IntentResolvedEvent | null;

  // 拆解确认（plan）状态
  /** 待确认计划的 id；确认后作为 `send_message_stream` 的 `plan_id`。 */
  planId: string | null;
  /** 确认卡里的意图行（每个可执行步骤一行）。 */
  planIntents: PlanConfirmIntent[];
  /** 拆解原始 payload，编辑/删除后用于重建意图名与探针证据。 */
  planPayloadIntents: DecomposedIntentPayload[];
  /** 本回合资源作用域快照（只读）；`null` 表示尚未拆解。 */
  planScope: ScopePayload | null;
  /** 最近一次拿到的计划快照（确认时写入 `activePlan`）。 */
  pendingPlan: ExecutionPlan | null;
  planConfirmOpen: boolean;
  planConfirmLoading: boolean;
  /** 等待确认的原始消息；取消即不发送。 */
  planDraftContent: string | null;

  // ── 会话日志（回合时间线）──
  /** 本回合 id；由 `beginTurn` 生成并贯穿拆解 / 流式 / 执行。 */
  activeTurnId: string | null;
  /** 当前回合实时条目（随 `session_log` 事件追加）。 */
  liveTurnLog: { turnId: string; entries: TurnLogEntry[] } | null;
  sessionLogs: TurnLogSummary[];
  sessionLogOpen: boolean;

  beginTurn: () => string;
  appendLiveLog: (payload: SessionLogEvent) => void;
  clearLiveLog: () => void;
  fetchSessionLogs: () => Promise<void>;
  openSessionLog: () => void;
  closeSessionLog: () => void;

  requestPlan: (content: string) => Promise<'chat' | 'confirm'>;
  confirmPlan: () => Promise<void>;
  cancelPlanConfirmation: () => Promise<void>;
  replanPlan: () => Promise<void>;
  updatePlanIntent: (stepId: string, instruction: string) => Promise<void>;
  removePlanIntent: (stepId: string) => Promise<void>;

  fetchSessions: () => Promise<void>;
  createSession: (title: string, modelId: string, systemPrompt?: string, personaId?: string, projectId?: string) => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  updateSessionTitle: (id: string, title: string) => Promise<void>;
  updateSessionModel: (id: string, modelId: string) => Promise<void>;
  clearSession: () => Promise<void>;
  newChat: () => void;
  sendMessage: (content: string) => Promise<void>;
  updateSessionConfig: (id: string, config: Record<string, unknown>) => Promise<void>;
  updateSessionWorkspace: (id: string, workspacePath: string | null) => Promise<void>;

  // Intent domain actions
  fetchSessionIntentState: (sessionId: string) => Promise<void>;
  setSessionIntentDomain: (sessionId: string, domainId: string | null) => Promise<void>;
  setCurrentTurnIntent: (intent: IntentResolvedEvent | null) => void;

  // Execution actions
  setSessionMode: (mode: SessionMode) => void;
  setExecutionStatus: (status: ExecStatus) => void;
  setActivePlan: (plan: ExecutionPlan | null) => void;
  setPlanProgress: (progress: PlanProgressEvent | null) => void;
  addExecutionLog: (entry: ExecutionLogEntry) => void;
  clearExecutionLogs: () => void;
  executePlan: (sessionId: string, plan: ExecutionPlan) => Promise<void>;
  pauseExecution: (sessionId: string) => Promise<void>;
  resumeExecution: (sessionId: string) => Promise<void>;
  cancelExecution: (sessionId: string) => Promise<void>;

  // ── Tools state (merged from toolSlice) ──
  tools: ToolInfo[];
  fetchTools: () => Promise<void>;
  toggleTool: (name: string, enabled: boolean) => Promise<void>;

  // ── System prompts state (merged from promptSlice) ──
  systemPrompts: SystemPrompt[];
  fetchSystemPrompts: () => Promise<void>;
  createSystemPrompt: (name: string, content: string, isDefault: boolean) => Promise<void>;
  deleteSystemPrompt: (id: string) => Promise<void>;
  setDefaultSystemPrompt: (id: string) => Promise<void>;
}

export const createSessionSlice: StateCreator<any, [], [], SessionSlice> = (set, get) => ({
  sessions: [],
  currentSession: null,
  messages: [],
  tools: [],
  systemPrompts: [],
  sessionMode: 'chat',
  executionStatus: { type: 'idle' },
  activePlan: null,
  planProgress: null,
  executionLogs: [],
  intentDomainId: null,
  intentDomainName: null,
  intentDomainMode: 'none',
  currentTurnIntent: null,
  planId: null,
  planIntents: [],
  planPayloadIntents: [],
  planScope: null,
  pendingPlan: null,
  planConfirmOpen: false,
  planConfirmLoading: false,
  planDraftContent: null,
  activeTurnId: null,
  liveTurnLog: null,
  sessionLogs: [],
  sessionLogOpen: false,

  fetchSessions: async () => {
    get().setLoading(true);
    try {
      const sessions = await api.listSessions(true);
      set({ sessions, loading: false });
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  createSession: async (title, modelId, systemPrompt, personaId, projectId) => {
    get().setLoading(true);
    try {
      const conv = await api.createSession(title, modelId, systemPrompt, personaId, projectId);
      await get().fetchSessionIntentState(conv.id);

      set((state: any) => ({
        sessions: [conv, ...state.sessions],
        currentSession: conv,
        messages: [],
        loading: false,
        error: null,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  selectSession: async (id) => {
    get().setLoading(true);
    try {
      const conv = await api.getSession(id);
      if (conv) {
        const messages = await api.getMessages(id);
        // Load persisted request context for this session
        const ctxJson = await api.getRequestContext(id);
        if (ctxJson) {
          try { get().setSessionMessages(JSON.parse(ctxJson)); } catch (err) { console.error('Failed to parse session messages:', err); }
        }
        set({ currentSession: conv, messages, currentTurnIntent: null, loading: false });
        await get().fetchSessionIntentState(id);
      }
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  deleteSession: async (id) => {
    get().setLoading(true);
    try {
      await api.deleteSession(id);
      const isCurrent = get().currentSession?.id === id;
      set((state: any) => ({
        sessions: state.sessions.filter((c: Session) => c.id !== id),
        currentSession: isCurrent ? null : state.currentSession,
        messages: isCurrent ? [] : state.messages,
        intentDomainId: isCurrent ? null : state.intentDomainId,
        intentDomainName: isCurrent ? null : state.intentDomainName,
        intentDomainMode: isCurrent ? 'none' : state.intentDomainMode,
        currentTurnIntent: isCurrent ? null : state.currentTurnIntent,
        planId: isCurrent ? null : state.planId,
        planIntents: isCurrent ? [] : state.planIntents,
        planScope: isCurrent ? null : state.planScope,
        planConfirmOpen: isCurrent ? false : state.planConfirmOpen,
        planDraftContent: isCurrent ? null : state.planDraftContent,
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  updateSessionTitle: async (id, title) => {
    try {
      await api.updateSessionTitle(id, title);
      set((state: any) => ({
        sessions: state.sessions.map((c: Session) =>
          c.id === id ? { ...c, title } : c
        ),
        currentSession: state.currentSession?.id === id
          ? { ...state.currentSession, title }
          : state.currentSession,
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  updateSessionModel: async (id, modelId) => {
    try {
      await api.updateSessionModel(id, modelId);
      set((state: any) => ({
        sessions: state.sessions.map((c: Session) =>
          c.id === id ? { ...c, model_id: modelId } : c
        ),
        currentSession: state.currentSession?.id === id
          ? { ...state.currentSession, model_id: modelId }
          : state.currentSession,
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  clearSession: async () => {
    const { currentSession } = get();
    if (!currentSession) return;
    try {
      await api.clearSession(currentSession.id);
      set({ messages: [] });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  newChat: () => {
    set({
      currentSession: null,
      messages: [],
      intentDomainId: null,
      intentDomainName: null,
      intentDomainMode: 'none',
      currentTurnIntent: null,
      planId: null,
      planIntents: [],
      planScope: null,
      planConfirmOpen: false,
      planDraftContent: null,
    });
  },

  sendMessage: async (content) => {
    const { currentSession } = get();
    if (!currentSession) return;

    const userMessage: Message = {
      id: Date.now().toString(),
      session_id: currentSession.id,
      role: 'user',
      content,
      created_at: new Date().toISOString(),
    };

    set((state: any) => ({
      messages: [...state.messages, userMessage],
      loading: true,
      error: null,
      currentTurnIntent: null,
    }));

    try {
      const assistantMsg = await api.sendMessage(currentSession.id, content);
      set((state: any) => ({
        messages: [...state.messages, assistantMsg],
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  updateSessionConfig: async (id, config) => {
    try {
      const configStr = JSON.stringify(config);
      await api.updateSessionConfig(id, configStr);
      set((state: any) => ({
        sessions: state.sessions.map((s: any) =>
          s.id === id ? { ...s, config: configStr } : s
        ),
        currentSession: state.currentSession?.id === id
          ? { ...state.currentSession, config: configStr }
          : state.currentSession,
      }));
    } catch (err) {
      console.error('Failed to update session config:', err);
    }
  },

  // ── Intent Domain Actions ──

  fetchSessionIntentState: async (sessionId) => {
    try {
      const state: SessionIntentState = await api.getSessionIntentState(sessionId);
      set({
        intentDomainId: state.domain_id,
        intentDomainName: state.domain_name,
        intentDomainMode: state.mode,
      });
    } catch (err) {
      console.error('Failed to fetch session intent state:', err);
    }
  },

  setSessionIntentDomain: async (sessionId, domainId) => {
    try {
      // 切换领域后旧的候选集合与待确认计划都失效：先丢弃计划，
      // 下一次发送时重新拆解（「触发重新规划」）。
      const stalePlanId = get().planId;
      if (stalePlanId) {
        try {
          await api.cancelPlan(stalePlanId);
        } catch (err) {
          console.error('Failed to cancel stale plan:', err);
        }
      }
      const state: SessionIntentState = await api.setSessionIntentDomain(sessionId, domainId);
      const session: Session | null = get().currentSession;
      set({
        // 后端已清除该会话的拆解缓存，本地同步，下一次发送重新拆解。
        currentSession: session ? { ...session, active_plan_id: null } : null,
        intentDomainId: state.domain_id,
        intentDomainName: state.domain_name,
        intentDomainMode: state.mode,
        currentTurnIntent: null,
        planId: null,
        planIntents: [],
        planPayloadIntents: [],
        planScope: null,
        pendingPlan: null,
        planConfirmOpen: false,
        planConfirmLoading: false,
        planDraftContent: null,
      });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setCurrentTurnIntent: (intent) => set({ currentTurnIntent: intent }),

  // ── 会话日志 ──

  beginTurn: () => {
    const turnId = typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : `turn-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
    set({ activeTurnId: turnId, liveTurnLog: { turnId, entries: [] } });
    return turnId;
  },

  appendLiveLog: (payload) => {
    // 跨会话事件直接丢弃：抽屉只看当前会话。
    if (payload.session_id !== get().currentSession?.id) return;
    set((state: any) => {
      const live = state.liveTurnLog;
      const entries = live && live.turnId === payload.turn_id
        ? [...live.entries, payload.entry]
        : [payload.entry];
      return { liveTurnLog: { turnId: payload.turn_id, entries } };
    });
  },

  clearLiveLog: () => set({ liveTurnLog: null }),

  fetchSessionLogs: async () => {
    const sessionId = get().currentSession?.id;
    if (!sessionId) return;
    try {
      const logs = await api.getSessionTurnLogs(sessionId);
      set({ sessionLogs: logs });
    } catch (err) {
      console.error('Failed to fetch session turn logs:', err);
    }
  },

  openSessionLog: () => {
    set({ sessionLogOpen: true });
    void get().fetchSessionLogs();
  },

  closeSessionLog: () => set({ sessionLogOpen: false }),

  // ── 拆解确认（plan）──

  requestPlan: async (content) => {
    const session: Session | null = get().currentSession;
    if (!session) return 'chat';

    // 会话已有拆解结果（active_plan_id）时，后续消息沿用意图上下文，
    // 不再重新拆解，也不弹确认卡（重新规划只能由显式操作触发）。
    if (session.active_plan_id) return 'chat';

    set({ planConfirmLoading: true, error: null });
    try {
      const turnId = get().activeTurnId ?? get().beginTurn();
      const result = await api.decomposeMessage(session.id, content, turnId);
      // 分解器不可用（超时 / 调用失败）：不弹确认卡，也不缓存——提示用户，
      // 本条消息照常按普通聊天发送，下一条消息会重试拆解。
      if (result.unavailable) {
        set({
          planConfirmLoading: false,
          error: '意图拆解服务暂时不可用，本条消息按普通聊天处理。',
        });
        return 'chat';
      }
      // 合法零意图：后端已落 0 步缓存计划，同步本地指向后后续消息不再拆解。
      if (!result.plan_id) {
        set({ planConfirmLoading: false });
        return 'chat';
      }
      if (result.intents.length === 0) {
        set({
          currentSession: { ...session, active_plan_id: result.plan_id },
          planConfirmLoading: false,
        });
        return 'chat';
      }

      // 计划详情提供 step_id（删步骤 / 改指令需要）；拿不到时降级为只读快照。
      let plan: ExecutionPlan | null = null;
      try {
        const detail = await api.getPlanDetail(result.plan_id);
        plan = detail?.plan ?? null;
      } catch (err) {
        console.error('Failed to load plan detail:', err);
      }

      set({
        currentSession: { ...session, active_plan_id: result.plan_id },
        planId: result.plan_id,
        planDraftContent: content,
        planPayloadIntents: result.intents,
        planScope: result.scope ?? null,
        planIntents: buildPlanConfirmIntents(plan, result.intents),
        pendingPlan: plan,
        planConfirmOpen: true,
        planConfirmLoading: false,
      });
      return 'confirm';
    } catch (err) {
      // 拆解失败不能让消息发不出去：退化为普通聊天。
      console.error('Failed to decompose message:', err);
      set({ planConfirmLoading: false });
      return 'chat';
    }
  },

  confirmPlan: async () => {
    const { planId, planDraftContent, pendingPlan } = get();
    if (!planId || !planDraftContent) return;

    // 确认后才把计划接上真实来源，供时间线与状态栏展示。
    set({
      activePlan: pendingPlan,
      executionStatus: { type: 'running', step_index: 0, started_at: new Date().toISOString() },
      planProgress: null,
      planConfirmOpen: false,
      planConfirmLoading: false,
      planId: null,
      planIntents: [],
      planPayloadIntents: [],
      planScope: null,
      pendingPlan: null,
      planDraftContent: null,
    });

    const personaId = (get().activePersonaId as string | null) ?? undefined;
    await get().sendMessageStream(planDraftContent, undefined, personaId, planId);
  },

  cancelPlanConfirmation: async () => {
    const { planId, currentSession } = get();
    if (planId) {
      try {
        await api.cancelPlan(planId);
      } catch (err) {
        console.error('Failed to cancel plan:', err);
      }
    }
    // 取消 = 这条消息根本不发出去；后端已解除会话指向，本地同步清空。
    set({
      currentSession: currentSession ? { ...currentSession, active_plan_id: null } : null,
      planConfirmOpen: false,
      planConfirmLoading: false,
      planId: null,
      planIntents: [],
      planPayloadIntents: [],
      planScope: null,
      pendingPlan: null,
      planDraftContent: null,
      currentTurnIntent: null,
      activeTurnId: null,
      liveTurnLog: null,
    });
  },

  replanPlan: async () => {
    const { planId, planDraftContent, currentSession } = get();
    if (!planDraftContent) return;

    if (planId) {
      try {
        await api.cancelPlan(planId);
      } catch (err) {
        console.error('Failed to cancel plan before replan:', err);
      }
    }
    set({
      // 旧计划已丢弃，本地缓存指向也要清空，否则 requestPlan 会当成「已有拆解结果」。
      currentSession: currentSession ? { ...currentSession, active_plan_id: null } : null,
      planConfirmOpen: false,
      planConfirmLoading: true,
      planId: null,
      planIntents: [],
      planPayloadIntents: [],
      planScope: null,
      pendingPlan: null,
      currentTurnIntent: null,
    });

    const outcome = await get().requestPlan(planDraftContent);
    if (outcome === 'chat') {
      // 重新拆解后零意图：按普通聊天发送，用户本来就打算发这条消息。
      const personaId = (get().activePersonaId as string | null) ?? undefined;
      await get().sendMessageStream(planDraftContent, undefined, personaId);
    }
  },

  updatePlanIntent: async (stepId, instruction) => {
    const { planId, planPayloadIntents } = get();
    if (!planId || !stepId) return;
    set({ planConfirmLoading: true });
    try {
      const plan: ExecutionPlan = await api.updatePlanStep(planId, stepId, instruction);
      set({
        pendingPlan: plan,
        planIntents: buildPlanConfirmIntents(plan, planPayloadIntents),
        planConfirmLoading: false,
      });
    } catch (err) {
      set({ error: String(err), planConfirmLoading: false });
    }
  },

  removePlanIntent: async (stepId) => {
    const { planId, planPayloadIntents } = get();
    if (!planId || !stepId) return;
    set({ planConfirmLoading: true });
    try {
      const plan: ExecutionPlan = await api.removePlanStep(planId, stepId);
      set({
        pendingPlan: plan,
        planIntents: buildPlanConfirmIntents(plan, planPayloadIntents),
        planConfirmLoading: false,
      });
    } catch (err) {
      set({ error: String(err), planConfirmLoading: false });
    }
  },

  // ── Execution Actions ──

  setSessionMode: (mode) => set({ sessionMode: mode }),

  setExecutionStatus: (status) => set({ executionStatus: status }),

  setActivePlan: (plan) => set({ activePlan: plan }),

  setPlanProgress: (progress) => set({ planProgress: progress }),

  addExecutionLog: (entry) => {
    // Add execution log as a visible message in the chat
    const sessionId = get().currentSession?.id;
    if (!sessionId) return;
    const logMsg: Message = {
      id: `exec-log-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      session_id: sessionId,
      role: 'tool',
      content: `[${entry.step}] ${entry.message}`,
      created_at: entry.timestamp,
    };
    set((state: any) => ({
      executionLogs: [...state.executionLogs, entry].slice(-200),
      messages: [...state.messages, logMsg],
    }));
  },

  updateSessionWorkspace: async (id, workspacePath) => {
    const state = get() as any;
    const session = state.sessions.find((s: any) => s.id === id);
    if (!session) return;
    const config = session.config ? (() => { try { return JSON.parse(session.config); } catch { return {} as Record<string, unknown>; } })() : {} as Record<string, unknown>;
    if (workspacePath) {
      config.workspace_path = workspacePath;
    } else {
      delete config.workspace_path;
    }
    await get().updateSessionConfig(id, config);
  },

  clearExecutionLogs: () => set({ executionLogs: [] }),

  executePlan: async (sessionId, plan) => {
    set({
      sessionMode: 'autonomous',
      executionStatus: { type: 'running', step_index: 0, started_at: new Date().toISOString() },
      activePlan: plan,
    });
    try {
      await api.executePlan(sessionId, JSON.stringify(plan), get().activeTurnId ?? undefined);
    } catch (err) {
      set({ executionStatus: { type: 'failed', step_index: 0, error: String(err) } });
    }
  },

  pauseExecution: async (sessionId) => {
    try {
      await api.pauseExecution(sessionId);
      set((state: any) => ({
        executionStatus: { type: 'paused', step_index: state.planProgress?.step_index ?? 0, reason: 'user_paused' },
      }));
    } catch (err) {
      console.error('Failed to pause:', err);
    }
  },

  resumeExecution: async (sessionId) => {
    try {
      await api.resumeExecution(sessionId);
    } catch (err) {
      console.error('Failed to resume:', err);
    }
  },

  cancelExecution: async (sessionId) => {
    try {
      await api.cancelExecution(sessionId);
      set({ activePlan: null, executionStatus: { type: 'idle' }, planProgress: null });
    } catch (err) {
      console.error('Failed to cancel:', err);
    }
  },

  // ── Tools (merged from toolSlice) ──

  fetchTools: async () => {
    try {
      const tools = await api.listTools();
      set({ tools });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  toggleTool: async (name, enabled) => {
    try {
      await api.toggleTool(name, enabled);
      set((state: any) => ({
        tools: state.tools.map((t: any) =>
          t.name === name ? { ...t, enabled } : t
        ),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  // ── System prompts (merged from promptSlice) ──

  fetchSystemPrompts: async () => {
    try {
      const prompts = await api.listSystemPrompts();
      set({ systemPrompts: prompts });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  createSystemPrompt: async (name, content, isDefault) => {
    try {
      const prompt = await api.createSystemPrompt(name, content, isDefault);
      set((state: any) => ({
        systemPrompts: [prompt, ...state.systemPrompts],
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  deleteSystemPrompt: async (id) => {
    try {
      await api.deleteSystemPrompt(id);
      set((state: any) => ({
        systemPrompts: state.systemPrompts.filter((p: any) => p.id !== id),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setDefaultSystemPrompt: async (id) => {
    try {
      await api.setDefaultSystemPrompt(id);
      set((state: any) => ({
        systemPrompts: state.systemPrompts.map((p: any) => ({
          ...p,
          is_default: p.id === id,
        })),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },
});
