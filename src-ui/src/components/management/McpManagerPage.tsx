import { useEffect, useRef, useState } from 'react';
import {
  PlugZap, Plug, Unplug, Trash2, Plus, Loader2,
  RefreshCw, Terminal, RotateCw, AlertTriangle,
  XCircle, Clock, Pause, X, Package, CheckCircle2,
} from 'lucide-react';
import { useStore } from '../../store';
import { ManagerPageLayout } from '../common/ManagerPageLayout';
import type { McpConnectionStatus } from '../../types';

// ── Status helpers ──

const STATUS_CONFIG: Record<McpConnectionStatus, { icon: typeof Plug; color: string; label: string }> = {
  ready:     { icon: CheckCircle2, color: 'text-green-600 bg-green-100 dark:bg-green-900/40', label: '运行中' },
  degraded:  { icon: AlertTriangle, color: 'text-amber-600 bg-amber-100 dark:bg-amber-900/40', label: '降级' },
  starting:  { icon: Loader2, color: 'text-blue-600 bg-blue-100 dark:bg-blue-900/40', label: '启动中' },
  waiting:   { icon: Clock, color: 'text-blue-600 bg-blue-100 dark:bg-blue-900/40', label: '等待中' },
  stopping:  { icon: Pause, color: 'text-gray-500 bg-gray-200 dark:bg-gray-600', label: '停止中' },
  stopped:   { icon: Unplug, color: 'text-gray-500 bg-gray-200 dark:bg-gray-600', label: '已断开' },
  disabled:  { icon: XCircle, color: 'text-gray-400 bg-gray-100 dark:bg-gray-700', label: '已禁用' },
  error:     { icon: AlertTriangle, color: 'text-red-600 bg-red-100 dark:bg-red-900/40', label: '错误' },
};

function getStatusConfig(status: McpConnectionStatus) {
  return STATUS_CONFIG[status] || STATUS_CONFIG.stopped;
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

// ── Log Viewer Modal ──

function LogViewerModal({ serverId, serverName }: { serverId: string; serverName: string }) {
  const { logEntries, logLoading, closeLogViewer, fetchMcpLogs } = useStore();
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logEntries]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl border border-gray-200 dark:border-gray-700 w-[640px] max-h-[480px] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <Terminal size={16} className="text-gray-400" />
            <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
              {serverName} — stderr 日志
            </span>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => fetchMcpLogs(serverId)}
              className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-purple-500 transition-colors"
              title="刷新"
            >
              <RefreshCw size={14} />
            </button>
            <button
              onClick={closeLogViewer}
              className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-gray-600 transition-colors"
            >
              <XCircle size={16} />
            </button>
          </div>
        </div>

        {/* Log content */}
        <div ref={scrollRef} className="flex-1 overflow-y-auto p-4 font-mono text-xs space-y-0.5 bg-gray-950 text-gray-300 rounded-b-2xl">
          {logLoading && (
            <div className="flex items-center gap-2 text-gray-500 py-2">
              <Loader2 size={12} className="animate-spin" />
              加载中...
            </div>
          )}
          {!logLoading && logEntries.length === 0 && (
            <div className="text-gray-500 py-2 text-center">暂无日志输出</div>
          )}
          {logEntries.map((line, i) => {
            const isError = line.includes('[error]') || line.includes('Error') || line.includes('ERR');
            const isWarn = line.includes('[warn]') || line.includes('WARN') || line.includes('Warning');
            return (
              <div
                key={i}
                className={`${isError ? 'text-red-400' : isWarn ? 'text-amber-400' : 'text-gray-300'}`}
              >
                {line}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ── Main Page ──

export function McpManagerPage() {
  const {
    mcpConnections, fetchMcpConnections, addMcpServer, removeMcpServer,
    connectMcpServer, disconnectMcpServer, mcpLoading, mcpError,
    addMcpDialogOpen, newMcpName, newMcpCommand, newMcpArgs,
    setNewMcpName, setNewMcpCommand, setNewMcpArgs, setAddMcpDialogOpen, clearMcpError,
    updateToolConfig, restartMcpServer,
    logViewerServerId, logViewerOpen, openLogViewer,
    mcpRuntimeSuggestion, mcpRuntimeChecking, setCurrentView,
  } = useStore();

  const [expandedConnId, setExpandedConnId] = useState<string | null>(null);

  // Fetch on mount + auto-refresh every 5 seconds
  useEffect(() => {
    fetchMcpConnections();
    const interval = setInterval(fetchMcpConnections, 5000);
    return () => clearInterval(interval);
  }, [fetchMcpConnections]);

  const handleViewLogs = async (id: string) => {
    await openLogViewer(id);
  };

  return (
    <ManagerPageLayout
      icon={<PlugZap size={20} className="text-white" />}
      title="MCP 连接管理"
      subtitle={`共 ${mcpConnections.length} 个服务器`}
      headerActions={
        <button
          onClick={() => setAddMcpDialogOpen(true)}
          className="flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white rounded-lg text-sm transition-all shadow-sm hover:shadow-md font-medium"
        >
          <Plus size={15} />
          添加 Server
        </button>
      }
    >
      <div className="max-w-3xl mx-auto space-y-3">
          {/* Empty state */}
          {mcpConnections.length === 0 && !mcpLoading && (
            <div className="p-8 text-center text-gray-400 dark:text-gray-500">
              <PlugZap size={32} className="mx-auto mb-2 opacity-50" />
              <p className="text-sm">暂无 MCP Server 连接</p>
              <p className="text-xs mt-1">添加一个 MCP Server 后，其工具将自动出现在 Agent 的工具列表中</p>
            </div>
          )}

          {/* Connection cards */}
          {mcpConnections.map((conn) => {
            const statusCfg = getStatusConfig(conn.status);
            const StatusIcon = statusCfg.icon;

            return (
              <div key={conn.id} className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
                {/* Top row */}
                <div className="flex items-center gap-4">
                  {/* Status icon */}
                  <div className={`w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0 ${statusCfg.color}`}>
                    {conn.status === 'starting' || conn.status === 'waiting' ? (
                      <Loader2 size={18} className="animate-spin" />
                    ) : (
                      <StatusIcon size={18} />
                    )}
                  </div>

                  {/* Name + Status */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <h4 className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">{conn.name}</h4>
                      <span className={`text-xs px-1.5 py-0.5 rounded-full ${
                        conn.status === 'ready' ? 'bg-green-100 text-green-600 dark:bg-green-900/40 dark:text-green-400' :
                        conn.status === 'degraded' ? 'bg-amber-100 text-amber-600 dark:bg-amber-900/40 dark:text-amber-400' :
                        conn.status === 'error' ? 'bg-red-100 text-red-600 dark:bg-red-900/40 dark:text-red-400' :
                        conn.status === 'starting' || conn.status === 'waiting' ? 'bg-blue-100 text-blue-600 dark:bg-blue-900/40 dark:text-blue-400' :
                        'bg-gray-200 text-gray-500 dark:bg-gray-600 dark:text-gray-400'
                      }`}>
                        {statusCfg.label}
                        {conn.status === 'ready' ? ` · ${conn.tool_count} 工具` : ''}
                      </span>
                    </div>

                    {/* Status detail (error message, etc.) */}
                    {conn.status_detail && (
                      <p className="text-xs text-red-500 dark:text-red-400 mt-1 truncate max-w-md">
                        {conn.status_detail}
                      </p>
                    )}

                    {/* Action buttons */}
                    <div className="flex items-center gap-1 mt-1">
                      <button
                        onClick={() => setExpandedConnId(expandedConnId === conn.id ? null : conn.id)}
                        className="text-xs text-purple-500 hover:text-purple-600"
                      >
                        {expandedConnId === conn.id ? '收起' : `工具 (${conn.tools?.length || 0} 个)`}
                      </button>
                      <span className="text-gray-300 dark:text-gray-600">·</span>
                      <button
                        onClick={() => handleViewLogs(conn.id)}
                        className="text-xs text-purple-500 hover:text-purple-600"
                      >
                        日志
                      </button>
                    </div>
                  </div>

                  {/* Action buttons */}
                  <div className="flex items-center gap-1 flex-shrink-0">
                    {conn.status === 'ready' || conn.status === 'degraded' ? (
                      <>
                        <button
                          onClick={() => restartMcpServer(conn.id)}
                          className="p-1.5 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-blue-500 transition-colors"
                          title="重启"
                        >
                          <RotateCw size={14} />
                        </button>
                        <button
                          onClick={() => disconnectMcpServer(conn.id)}
                          className="p-1.5 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-red-500 transition-colors"
                          title="断开"
                        >
                          <Unplug size={14} />
                        </button>
                      </>
                    ) : conn.status === 'starting' || conn.status === 'waiting' ? (
                      <span className="text-xs text-gray-400 px-2">连接中...</span>
                    ) : (
                      <button
                        onClick={() => connectMcpServer(conn.id)}
                        className="p-1.5 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-green-500 transition-colors"
                        title="连接"
                      >
                        <Plug size={14} />
                      </button>
                    )}
                    <button
                      onClick={() => removeMcpServer(conn.id)}
                      className="p-1.5 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-red-500 transition-colors"
                      title="移除"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>

                {/* Stats bar (when ready or degraded) */}
                {(conn.status === 'ready' || conn.status === 'degraded') && (
                  <div className="mt-2 pt-2 border-t border-gray-100 dark:border-gray-600">
                    <div className="flex gap-4 text-[10px] text-gray-400">
                      <span className="flex items-center gap-1">
                        <Clock size={10} /> {formatUptime(conn.stats.uptime_seconds)}
                      </span>
                      <span>调用 {conn.stats.total_calls} 次</span>
                      <span className={conn.stats.error_count > 0 ? 'text-red-400' : ''}>
                        错误 {conn.stats.error_count} 次
                      </span>
                      <span>延迟 {conn.stats.avg_latency_ms?.toFixed(0)}ms</span>
                    </div>
                  </div>
                )}



                {/* Expandable tools */}
                {expandedConnId === conn.id && conn.tools?.length > 0 && (
                  <div className="mt-3 pt-3 border-t border-gray-100 dark:border-gray-600 space-y-1">
                    {conn.tools.map((tool) => (
                      <div key={tool.name} className="flex items-center gap-2 px-2 py-1.5 rounded bg-gray-100/50 dark:bg-gray-700/50 text-xs">
                        <span className="flex-1 truncate text-gray-700 dark:text-gray-300">{tool.name}</span>
                        <label className="relative inline-flex items-center cursor-pointer">
                          <input
                            type="checkbox"
                            checked={tool.enabled}
                            onChange={() => {
                              updateToolConfig(conn.id, tool.name, !tool.enabled, tool.confirmation);
                            }}
                            className="sr-only peer"
                          />
                          <div className="w-8 h-4 bg-gray-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[0.5px] after:left-[0.5px] after:bg-white after:rounded-full after:h-3 after:w-3 after:transition-all peer-checked:bg-purple-600"></div>
                        </label>
                        <span className={`text-[10px] px-1 py-0.5 rounded ${
                          tool.confirmation === 'auto_allow' ? 'bg-green-100 text-green-600' :
                          tool.confirmation === 'confirm_once' ? 'bg-amber-100 text-amber-600' : 'bg-red-100 text-red-600'
                        }`}>
                          {tool.confirmation === 'auto_allow' ? '自动' : tool.confirmation === 'confirm_once' ? '确认' : '拒绝'}
                        </span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}

          {/* Loading */}
          {mcpLoading && (
            <div className="flex items-center justify-center py-4 text-gray-400">
              <Loader2 size={16} className="animate-spin mr-2" />
              <span className="text-sm">处理中...</span>
            </div>
          )}

          {/* Error banner */}
          {mcpError && (
            <div className="p-3 bg-red-50 dark:bg-red-900/20 border border-red-100 dark:border-red-800 rounded-lg text-sm text-red-600 dark:text-red-400 flex items-center justify-between">
              <span>{mcpError}</span>
              <button onClick={clearMcpError} className="underline text-xs ml-2">关闭</button>
            </div>
          )}

      </div>

      {logViewerOpen && logViewerServerId && (
        <LogViewerModal
          serverId={logViewerServerId}
          serverName={mcpConnections.find(c => c.id === logViewerServerId)?.name || ''}
        />
      )}

      {/* Add MCP Server Dialog */}
      {addMcpDialogOpen && (
        <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 rounded-2xl w-[480px] shadow-2xl animate-in fade-in zoom-in-95 duration-200">
            <div className="flex items-center justify-between p-5 border-b border-gray-100 dark:border-gray-700">
              <div className="flex items-center gap-2">
                <div className="w-8 h-8 rounded-lg bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center">
                  <Package size={16} className="text-purple-600 dark:text-purple-400" />
                </div>
                <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">添加 MCP Server</h2>
              </div>
              <button
                onClick={() => setAddMcpDialogOpen(false)}
                className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
              >
                <X size={18} />
              </button>
            </div>

            <div className="p-5 space-y-4">
              <div>
                <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">名称</label>
                <input
                  type="text"
                  value={newMcpName}
                  onChange={e => setNewMcpName(e.target.value)}
                  placeholder="给这个 MCP Server 起个名字"
                  className="w-full bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
                />
              </div>
              <div>
                <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">命令</label>
                <input
                  type="text"
                  value={newMcpCommand}
                  onChange={e => setNewMcpCommand(e.target.value)}
                  placeholder="如: npx, python, node"
                  className="w-full bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
                />
              </div>
              <div>
                <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">参数</label>
                <input
                  type="text"
                  value={newMcpArgs}
                  onChange={e => setNewMcpArgs(e.target.value)}
                  placeholder="如: -y @anthropic/mcp-server-filesystem"
                  className="w-full bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
                />
              </div>

              {mcpError && (
                <div className="p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-700 dark:text-red-400 flex items-start gap-2">
                  <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" />
                  <span>{mcpError}</span>
                </div>
              )}

              {/* Runtime hint */}
              {newMcpCommand.trim() && (
                <>
                  {mcpRuntimeChecking ? (
                    <div className="flex items-center gap-1.5 text-xs text-gray-400">
                      <Loader2 size={12} className="animate-spin" />
                      检测运行时环境...
                    </div>
                  ) : mcpRuntimeSuggestion?.runtime_type ? (
                    <div className={`flex items-center gap-1.5 text-xs px-2 py-1.5 rounded-lg ${
                      mcpRuntimeSuggestion.available
                        ? 'bg-green-50 dark:bg-green-900/20 text-green-600 dark:text-green-400'
                        : 'bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-400'
                    }`}>
                      {mcpRuntimeSuggestion.available ? (
                        <CheckCircle2 size={12} />
                      ) : (
                        <AlertTriangle size={12} />
                      )}
                      <span>
                        {mcpRuntimeSuggestion.display_name}
                        {mcpRuntimeSuggestion.available
                          ? ` ${mcpRuntimeSuggestion.version || ''} — 可用`
                          : ' — 未安装'}
                      </span>
                      {!mcpRuntimeSuggestion.available && (
                        <button
                          onClick={() => setCurrentView('runtime-manager')}
                          className="underline ml-1 hover:text-purple-600"
                        >
                          去安装
                        </button>
                      )}
                    </div>
                  ) : null}
                </>
              )}

              <div className="flex gap-2 pt-2">
                <button
                  onClick={addMcpServer}
                  disabled={!newMcpName.trim() || !newMcpCommand.trim() || mcpLoading}
                  className="flex-1 flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2.5 rounded-lg text-sm transition-all font-medium"
                >
                  {mcpLoading ? (
                    <><Loader2 size={14} className="animate-spin" /> 连接中...</>
                  ) : (
                    <><PlugZap size={14} /> 连接</>
                  )}
                </button>
                <button
                  onClick={() => setAddMcpDialogOpen(false)}
                  className="px-4 py-2.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-colors"
                >
                  取消
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </ManagerPageLayout>
  );
}
