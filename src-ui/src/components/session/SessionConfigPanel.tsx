import { useState, useEffect, useRef } from 'react';
import { Select, Switch } from 'antd';
import { Sparkles, Wrench, Server, FolderOpen, X, Puzzle } from 'lucide-react';
import { useStore } from '../../store';
import type { ToolInfo } from '../../types';

interface SessionConfigPanelProps {
  onStart: (config: { modelId: string; personaId: string | null; enabledTools: string[]; workspacePath?: string | null }) => void;
  onCancel: () => void;
}

export function SessionConfigPanel({ onStart, onCancel }: SessionConfigPanelProps) {
  const models = useStore((s) => s.models);
  const personas = useStore((s) => s.personas);
  const tools = useStore((s) => s.tools);
  const mcpConnections = useStore((s) => s.mcpConnections);
  const defaultModel = useStore((s) => s.defaultModel);
  const fetchModels = useStore((s) => s.fetchModels);
  const fetchPersonas = useStore((s) => s.fetchPersonas);
  const fetchTools = useStore((s) => s.fetchTools);
  const fetchMcpConnections = useStore((s) => s.fetchMcpConnections);

  const [selectedModel, setSelectedModel] = useState(defaultModel || '');
  const [selectedPersona, setSelectedPersona] = useState<string | null>(null);
  const [enabledTools, setEnabledTools] = useState<Set<string>>(new Set(
    tools.filter(t => t.enabled).map(t => t.name)
  ));
  const [workspacePath, setWorkspacePath] = useState<string | null>(null);
  const [expandedSection, setExpandedSection] = useState<string | null>(null);
  const [expandedTool, setExpandedTool] = useState<string | null>(null);
  const workspaceInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (models.length === 0) fetchModels();
    if (personas.length === 0) fetchPersonas();
    if (tools.length === 0) fetchTools();
    if (mcpConnections.length === 0) fetchMcpConnections();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const toggleTool = (name: string) => {
    setEnabledTools(prev => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const enabledModels = models.filter(m => m.enabled);

  return (
    <div className="flex flex-col items-center justify-center p-8 bg-gradient-to-b from-purple-50/50 to-white dark:from-gray-800/50 dark:to-gray-900 h-full overflow-y-auto">
      <div className="max-w-lg w-full">
        <div className="text-center mb-6">
          <div className="w-14 h-14 mx-auto mb-3 rounded-2xl bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center shadow-lg">
            <Sparkles size={28} className="text-white" />
          </div>
          <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100 mb-1">新建对话</h1>
          <p className="text-xs text-gray-500 dark:text-gray-400">配置本次对话的参数后开始</p>
        </div>

        <div className="space-y-3">
          {/* Model selection */}
          <div className="p-3 bg-white dark:bg-gray-800 rounded-xl border border-gray-100 dark:border-gray-700">
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 uppercase tracking-wider">模型</label>
            <Select
              value={selectedModel || undefined}
              onChange={setSelectedModel}
              className="w-full"
              placeholder="选择模型..."
              size="small"
              options={enabledModels.map(m => ({
                value: m.id,
                label: `${m.display_name} (${m.provider})`
              }))}
            />
            {enabledModels.length === 0 && (
              <p className="text-xs text-red-500 mt-1">请先在设置中配置模型提供商</p>
            )}
          </div>

          {/* Persona selection */}
          <div className="p-3 bg-white dark:bg-gray-800 rounded-xl border border-gray-100 dark:border-gray-700">
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 uppercase tracking-wider">
              虚拟人 <span className="font-normal normal-case">(可选)</span>
            </label>
            <Select
              value={selectedPersona}
              onChange={setSelectedPersona}
              className="w-full"
              placeholder="不使用虚拟人"
              allowClear
              size="small"
              options={personas.map(p => ({
                value: p.id,
                label: `${p.emoji} ${p.name} — ${p.title}`
              }))}
            />
            {selectedPersona && (
              <p className="text-xs text-gray-400 mt-1">
                虚拟人会覆盖系统提示词和模型选择
              </p>
            )}
          </div>

          {/* Tools section — collapsible */}
          <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-100 dark:border-gray-700 overflow-hidden">
            <button
              onClick={() => setExpandedSection(expandedSection === 'tools' ? null : 'tools')}
              className="w-full flex items-center gap-2 px-3 py-2.5 text-xs font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
            >
              <Wrench size={14} className="text-purple-500" />
              <span>内置工具</span>
              <span className="ml-auto text-gray-400">{enabledTools.size} 个已启用</span>
            </button>
            {expandedSection === 'tools' && (
              <div className="px-3 pb-2 space-y-1 border-t border-gray-100 dark:border-gray-700 pt-2">
                {tools.map(tool => (
                  <div key={tool.name} className="py-1">
                    <div className="flex items-center justify-between gap-2">
                      <button
                        type="button"
                        onClick={() =>
                          setExpandedTool(expandedTool === tool.name ? null : tool.name)
                        }
                        className="flex flex-wrap items-center gap-1 text-left"
                      >
                        <span className="text-xs text-gray-700 dark:text-gray-300 capitalize">
                          {tool.name.replace(/_/g, ' ')}
                        </span>
                        <ToolRiskBadges tool={tool} />
                      </button>
                      <Switch
                        size="small"
                        checked={enabledTools.has(tool.name)}
                        onChange={() => toggleTool(tool.name)}
                      />
                    </div>
                    {expandedTool === tool.name && (
                      <div className="mt-1.5 space-y-1.5 pl-1">
                        <p className="text-[11px] text-gray-500 dark:text-gray-400">
                          {tool.description}
                        </p>
                        <ToolParameters tool={tool} />
                        <p className="text-[10px] text-gray-400">
                          来源：{tool.source === 'builtin' ? '内置' : tool.source}
                        </p>
                      </div>
                    )}
                  </div>
                ))}
                {tools.length === 0 && (
                  <p className="text-xs text-gray-400 py-2">暂无工具</p>
                )}
                <p className="text-[10px] text-gray-400 pt-1 border-t border-gray-100 dark:border-gray-700">
                  未启用可能来自全局开关；MCP 工具由服务器配置决定；意图白名单在每轮路由时生效，无法在此固定。
                </p>
              </div>
            )}
          </div>

          {/* Skills hint */}
          <div className="p-3 bg-purple-50 dark:bg-purple-900/10 border border-purple-100 dark:border-purple-800/30 rounded-xl">
            <div className="flex items-start gap-2">
              <Puzzle size={14} className="text-purple-500 mt-0.5 flex-shrink-0" />
              <p className="text-xs text-purple-800 dark:text-purple-200">
                技能管理已移动到「技能管理」模块。安装的技能会按范围自动生效。
              </p>
            </div>
          </div>

          {/* Workspace section */}
          <div className="p-3 bg-white dark:bg-gray-800 rounded-xl border border-gray-100 dark:border-gray-700">
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 uppercase tracking-wider">
              工作空间 <span className="font-normal normal-case">(可选)</span>
            </label>
            <div className="flex items-center gap-2">
              <input
                ref={workspaceInputRef}
                type="text"
                value={workspacePath || ''}
                onChange={(e) => setWorkspacePath(e.target.value || null)}
                placeholder="输入项目路径或留空…"
                className="flex-1 px-3 py-1.5 text-xs border border-gray-200 dark:border-gray-600 rounded-lg bg-gray-50 dark:bg-gray-700 text-gray-800 dark:text-gray-200 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500/40 focus:border-purple-500"
              />
              <button
                onClick={async () => {
                  try {
                    const { open } = await import('@tauri-apps/plugin-dialog');
                    const selected = await open({ directory: true, multiple: false, title: '选择工作空间目录' });
                    if (selected) setWorkspacePath(selected as string);
                  } catch {
                    workspaceInputRef.current?.focus();
                  }
                }}
                className="p-1.5 text-gray-400 hover:text-purple-600 hover:bg-purple-50 dark:hover:bg-purple-900/30 rounded-lg transition-colors"
                title="浏览选择目录"
              >
                <FolderOpen size={16} />
              </button>
              {workspacePath && (
                <button
                  onClick={() => setWorkspacePath(null)}
                  className="p-1.5 text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
                  title="清除工作空间"
                >
                  <X size={16} />
                </button>
              )}
            </div>
            {workspacePath && (
              <p className="text-[10px] text-gray-400 mt-1 truncate">{workspacePath}</p>
            )}
          </div>

          {/* MCP section — collapsible */}
          <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-100 dark:border-gray-700 overflow-hidden">
            <button
              onClick={() => setExpandedSection(expandedSection === 'mcp' ? null : 'mcp')}
              className="w-full flex items-center gap-2 px-3 py-2.5 text-xs font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
            >
              <Server size={14} className="text-purple-500" />
              <span>MCP 服务器</span>
              <span className="ml-auto text-gray-400">{mcpConnections.length} 个连接</span>
            </button>
            {expandedSection === 'mcp' && (
              <div className="px-3 pb-2 border-t border-gray-100 dark:border-gray-700 pt-2 space-y-2">
                {mcpConnections.map(conn => (
                  <div key={conn.id}>
                    <div className="flex items-center justify-between py-1">
                      <span className="text-xs font-medium text-gray-700 dark:text-gray-300">{conn.name}</span>
                      <span className={`text-[10px] px-1.5 py-0.5 rounded ${
                        conn.status === 'ready' ? 'bg-green-100 text-green-700' : 'bg-gray-100 text-gray-500'
                      }`}>{conn.status}</span>
                    </div>
                    {conn.tools.map(mcpTool => (
                      <div key={mcpTool.name} className="flex items-center justify-between py-0.5 pl-3">
                        <span className="text-[11px] text-gray-500 dark:text-gray-400">{mcpTool.name}</span>
                        <Switch
                          size="small"
                          checked={enabledTools.has(mcpTool.name)}
                          onChange={() => toggleTool(mcpTool.name)}
                        />
                      </div>
                    ))}
                  </div>
                ))}
                {mcpConnections.length === 0 && (
                  <p className="text-xs text-gray-400 py-2">暂无 MCP 连接</p>
                )}
              </div>
            )}
          </div>

          {/* Action buttons */}
          <div className="flex gap-3 pt-1">
            <button
              onClick={onCancel}
              className="flex-1 px-4 py-2.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-xl text-sm font-medium transition-all"
            >
              取消
            </button>
            <button
              onClick={() => onStart({
                modelId: selectedModel,
                personaId: selectedPersona,
                enabledTools: Array.from(enabledTools),
                workspacePath,
              })}
              disabled={!selectedModel}
              className="flex-1 px-4 py-2.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-40 text-white rounded-xl text-sm font-medium transition-all shadow-sm"
            >
              开始对话
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/** 工具风险徽标：只读 / 可写 / 外部。 */
function ToolRiskBadges({ tool }: { tool: ToolInfo }) {
  const badges: Array<{ label: string; className: string }> = [];
  if (tool.read_only) {
    badges.push({
      label: '只读',
      className: 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-300',
    });
  }
  if (tool.destructive) {
    badges.push({
      label: '可写',
      className: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300',
    });
  }
  if (tool.open_world) {
    badges.push({
      label: '外部',
      className: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300',
    });
  }

  return (
    <>
      {badges.map((badge) => (
        <span key={badge.label} className={`text-[10px] px-1 py-0.5 rounded ${badge.className}`}>
          {badge.label}
        </span>
      ))}
    </>
  );
}

/** 参数表：名称 / 类型 / 是否必填。 */
function ToolParameters({ tool }: { tool: ToolInfo }) {
  const schema = tool.parameters as {
    properties?: Record<string, { type?: string; description?: string }>;
    required?: string[];
  };
  const properties = schema.properties ?? {};
  const required = new Set(schema.required ?? []);
  const entries = Object.entries(properties);

  if (entries.length === 0) {
    return <p className="text-[10px] text-gray-400">无参数</p>;
  }

  return (
    <div className="rounded border border-gray-100 dark:border-gray-700 overflow-hidden">
      <table className="w-full text-[10px]">
        <thead className="bg-gray-50 dark:bg-gray-800 text-gray-400">
          <tr>
            <th className="text-left px-2 py-1 font-medium">参数</th>
            <th className="text-left px-2 py-1 font-medium">类型</th>
            <th className="text-left px-2 py-1 font-medium">必填</th>
          </tr>
        </thead>
        <tbody>
          {entries.map(([name, property]) => (
            <tr key={name} className="border-t border-gray-100 dark:border-gray-700">
              <td className="px-2 py-1 font-mono text-gray-700 dark:text-gray-300">{name}</td>
              <td className="px-2 py-1 text-gray-500">{property.type ?? 'any'}</td>
              <td className="px-2 py-1 text-gray-500">{required.has(name) ? '是' : '否'}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
