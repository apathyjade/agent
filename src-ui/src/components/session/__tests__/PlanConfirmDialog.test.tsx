import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { PlanConfirmDialog, planIntentSideEffects, planScopeStatusLabel } from '../PlanConfirmDialog';
import type { PlanConfirmIntent } from '../../../api/plan';

// antd 的 TextArea 在挂载时会观察尺寸；jsdom 没有 ResizeObserver，补一个空实现。
if (!('ResizeObserver' in globalThis)) {
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  Object.assign(globalThis, { ResizeObserver: ResizeObserverStub });
}

function makeIntent(overrides: Partial<PlanConfirmIntent> = {}): PlanConfirmIntent {
  return {
    step_id: 'step-1',
    intent_id: 'test.a',
    intent_name: '实现功能',
    instruction: '实现 CSV 导出，包含表头与编码处理',
    confidence: 0.42,
    uncertain: true,
    allowed_tools: ['read', 'shell', 'file_system'],
    model_id: 'model-a',
    probe_evidence: null,
    ...overrides,
  };
}

function renderDialog(overrides: Partial<React.ComponentProps<typeof PlanConfirmDialog>> = {}) {
  const handlers = {
    onConfirm: vi.fn(),
    onCancel: vi.fn(),
    onReplan: vi.fn(),
    onRemoveIntent: vi.fn(),
    onUpdateInstruction: vi.fn(),
  };
  const props = {
    open: true,
    planId: 'plan-1',
    intents: [makeIntent()],
    loading: false,
    ...handlers,
    ...overrides,
  };
  render(<PlanConfirmDialog {...props} />);
  return handlers;
}

describe('planIntentSideEffects', () => {
  it('flags commands and file writes from the tool whitelist', () => {
    expect(planIntentSideEffects(['read', 'shell', 'file_system', 'code_executor'])).toEqual([
      '将执行命令',
      '将写入文件',
      '将执行代码',
    ]);
  });

  it('returns nothing for a read-only whitelist', () => {
    expect(planIntentSideEffects(['read', 'search'])).toEqual([]);
  });
});

describe('PlanConfirmDialog', () => {
  it('shows intent name, full instruction, tools, model and badges', () => {
    renderDialog();

    expect(screen.getByText('实现功能')).toBeTruthy();
    expect(screen.getByText('实现 CSV 导出，包含表头与编码处理')).toBeTruthy();
    expect(screen.getByText(/read · shell · file_system/)).toBeTruthy();
    expect(screen.getByText(/model-a/)).toBeTruthy();
    expect(screen.getByText('置信度低')).toBeTruthy();
    expect(screen.getByText(/将执行命令/)).toBeTruthy();
    expect(screen.getByText(/将写入文件/)).toBeTruthy();
  });

  it('renders nothing when closed', () => {
    renderDialog({ open: false });

    expect(screen.queryByText('实现功能')).toBeNull();
  });

  it('confirms with the current plan', async () => {
    const user = userEvent.setup();
    const handlers = renderDialog();

    await user.click(screen.getByRole('button', { name: '确认执行' }));

    expect(handlers.onConfirm).toHaveBeenCalled();
  });

  it('cancels without confirming', async () => {
    const user = userEvent.setup();
    const handlers = renderDialog();

    await user.click(screen.getByRole('button', { name: '取消' }));

    expect(handlers.onCancel).toHaveBeenCalled();
    expect(handlers.onConfirm).not.toHaveBeenCalled();
  });

  it('removes an intent step', async () => {
    const user = userEvent.setup();
    const handlers = renderDialog({
      intents: [
        makeIntent({ step_id: 'step-1', intent_id: 'test.a', intent_name: '实现功能' }),
        makeIntent({ step_id: 'step-2', intent_id: 'test.b', intent_name: '编写测试' }),
      ],
    });

    await user.click(screen.getAllByRole('button', { name: '删除该意图' })[0]);

    expect(handlers.onRemoveIntent).toHaveBeenCalledWith('step-1');
  });

  it('rewrites an intent instruction', async () => {
    const user = userEvent.setup();
    const handlers = renderDialog();

    await user.click(screen.getByRole('button', { name: '改写指令' }));
    const textarea = screen.getByLabelText('意图指令');
    await user.clear(textarea);
    await user.type(textarea, '改成导出 markdown');
    await user.click(screen.getByRole('button', { name: '保存指令' }));

    expect(handlers.onUpdateInstruction).toHaveBeenCalledWith('step-1', '改成导出 markdown');
  });

  it('replans the whole plan', async () => {
    const user = userEvent.setup();
    const handlers = renderDialog();

    await user.click(screen.getByRole('button', { name: '重新规划' }));

    expect(handlers.onReplan).toHaveBeenCalled();
  });

  it('shows the resolved scope read-only when provided', () => {
    renderDialog({
      scope: {
        status: 'applied',
        cached: true,
        tools: ['builtin:read', 'builtin:shell'],
        prompts: ['intent.dev'],
        memory_limit: 5,
        truncated: true,
      },
    });

    const block = screen.getByTestId('plan-scope');
    expect(block.textContent).toContain('已按请求收窄');
    expect(block.textContent).toContain('（缓存命中）');
    expect(block.textContent).toContain('已按预算截断');
    expect(block.textContent).toContain('builtin:read · builtin:shell');
    expect(block.textContent).toContain('intent.dev');
    expect(block.textContent).toContain('5 条');
  });

  it('explains why the scope was not narrowed', () => {
    renderDialog({
      scope: {
        status: 'skipped_small_catalog',
        cached: false,
        tools: [],
        prompts: [],
        memory_limit: null,
        truncated: false,
      },
    });

    const block = screen.getByTestId('plan-scope');
    expect(screen.getByTestId('plan-scope-status').textContent).toBe('候选资源太少，未收窄');
    expect(block.textContent).toContain('未限定');
    expect(block.textContent).toContain('默认');
  });

  it('omits the scope block when no scope is available', () => {
    renderDialog({ scope: null });

    expect(screen.queryByTestId('plan-scope')).toBeNull();
  });

  it('keeps unknown scope statuses visible', () => {
    expect(planScopeStatusLabel('future_status')).toBe('future_status');
  });
});
