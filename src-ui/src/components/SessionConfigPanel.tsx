import { useState, useEffect } from 'react';
import { Select, Switch } from 'antd';
import { Sparkles, Wrench, BrainCircuit, Server } from 'lucide-react';
import { useStore } from '../store';

interface SessionConfigPanelProps {
  onStart: (config: { modelId: string; personaId: string | null; enabledTools: string[] }) => void;
  onCancel: () => void;
}

export function SessionConfigPanel({ onStart, onCancel }: SessionConfigPanelProps) {
  const models = useStore((s) => s.models);
  const personas = useStore((s) => s.personas);
  const tools = useStore((s) => s.tools);
  const skills = useStore((s) => s.skills);
  const mcpConnections = useStore((s) => s.mcpConnections);
  const defaultModel = useStore((s) => s.defaultModel);
  const fetchModels = useStore((s) => s.fetchModels);
  const fetchPersonas = useStore((s) => s.fetchPersonas);
  const fetchTools = useStore((s) => s.fetchTools);
  const fetchSkills = useStore((s) => s.fetchSkills);
  const fetchMcpConnections = useStore((s) => s.fetchMcpConnections);

  const [selectedModel, setSelectedModel] = useState(defaultModel || '');
  const [selectedPersona, setSelectedPersona] = useState<string | null>(null);
  const [enabledTools, setEnabledTools] = useState<Set<string>>(new Set(
    tools.filter(t => t.enabled).map(t => t.name)
  ));
  const [expandedSection, setExpandedSection] = useState<string | null>(null);

  useEffect(() => {
    if (models.length === 0) fetchModels();
    if (personas.length === 0) fetchPersonas();
    if (tools.length === 0) fetchTools();
    if (skills.length === 0) fetchSkills();
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
                  <div key={tool.name} className="flex items-center justify-between py-1">
                    <span className="text-xs text-gray-700 dark:text-gray-300 capitalize">{tool.name.replace(/_/g, ' ')}</span>
                    <Switch
                      size="small"
                      checked={enabledTools.has(tool.name)}
                      onChange={() => toggleTool(tool.name)}
                    />
                  </div>
                ))}
                {tools.length === 0 && (
                  <p className="text-xs text-gray-400 py-2">暂无工具</p>
                )}
              </div>
            )}
          </div>

          {/* Skills section — collapsible */}
          <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-100 dark:border-gray-700 overflow-hidden">
            <button
              onClick={() => setExpandedSection(expandedSection === 'skills' ? null : 'skills')}
              className="w-full flex items-center gap-2 px-3 py-2.5 text-xs font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
            >
              <BrainCircuit size={14} className="text-purple-500" />
              <span>技能 (Skills)</span>
              <span className="ml-auto text-gray-400">{skills.filter(s => s.enabled).length} 个可用</span>
            </button>
            {expandedSection === 'skills' && (
              <div className="px-3 pb-2 space-y-1 border-t border-gray-100 dark:border-gray-700 pt-2">
                {skills.filter(s => s.enabled).map(skill => (
                  <div key={skill.id} className="flex items-center justify-between py-1">
                    <div className="flex-1 min-w-0">
                      <span className="text-xs text-gray-700 dark:text-gray-300">{skill.name}</span>
                      <p className="text-[10px] text-gray-400 truncate">{skill.description}</p>
                    </div>
                    <Switch
                      size="small"
                      checked={enabledTools.has(skill.name)}
                      onChange={() => toggleTool(skill.name)}
                    />
                  </div>
                ))}
                {skills.filter(s => s.enabled).length === 0 && (
                  <p className="text-xs text-gray-400 py-2">暂无已启用的技能，请在设置中安装</p>
                )}
              </div>
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
