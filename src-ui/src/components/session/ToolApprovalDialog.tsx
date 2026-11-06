import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { Button, Modal } from 'antd';
import { ShieldAlert } from 'lucide-react';
import type { ToolApprovalRequest } from '../../types';

/**
 * 工具审批弹窗。
 *
 * 并发产生的多个审批请求按到达顺序排队，逐个展示，互不覆盖。
 * 弹窗禁止 ESC / 点击遮罩关闭：必须明确选择允许或拒绝。
 */
export function ToolApprovalDialog() {
  const [queue, setQueue] = useState<ToolApprovalRequest[]>([]);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;

    listen<ToolApprovalRequest>('tool_approval_request', (event) => {
      setQueue((current) => [...current, event.payload]);
    })
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {
        // 事件通道不可用时不影响其它功能。
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const current = queue[0];

  const respond = useCallback(
    async (approved: boolean) => {
      if (!current || submitting) {
        return;
      }
      setSubmitting(true);
      try {
        await invoke('respond_tool_approval', { id: current.id, approved });
      } catch {
        // 请求可能已超时；无论结果如何都收起当前弹窗，继续下一个。
      } finally {
        setSubmitting(false);
        setQueue((rest) => rest.slice(1));
      }
    },
    [current, submitting],
  );

  return (
    <Modal
      open={Boolean(current)}
      title={
        <span className="flex items-center gap-2">
          <ShieldAlert size={16} className="text-amber-500" />
          需要确认工具调用
        </span>
      }
      closable={false}
      maskClosable={false}
      keyboard={false}
      footer={[
        <Button key="deny" danger disabled={submitting} onClick={() => respond(false)}>
          拒绝
        </Button>,
        <Button key="allow" type="primary" loading={submitting} onClick={() => respond(true)}>
          允许
        </Button>,
      ]}
    >
      {current && (
        <div className="space-y-3">
          <div className="text-sm text-gray-600 dark:text-gray-300">
            模型请求调用工具{' '}
            <code className="px-1 py-0.5 rounded bg-gray-100 dark:bg-gray-800">
              {current.tool_name}
            </code>
            。{current.reason}
          </div>
          <pre className="max-h-64 overflow-auto rounded bg-gray-50 dark:bg-gray-800 p-3 text-xs">
            {JSON.stringify(current.arguments, null, 2)}
          </pre>
          {queue.length > 1 && (
            <div className="text-xs text-gray-400">还有 {queue.length - 1} 个请求在排队</div>
          )}
        </div>
      )}
    </Modal>
  );
}
