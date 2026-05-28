import { useEffect, useState } from 'react';
import { Workflow, Play, Clock, Sparkles, Loader2, X, AlertTriangle } from 'lucide-react';
import { useStore } from '../../store';
import { ManagerPageLayout } from '../common/ManagerPageLayout';

export function WorkflowManagerPage() {
  const {
    workflows, fetchWorkflows, runWorkflow, workflowLoading, workflowResult, workflowError,
    workflowRuns, clearWorkflowResult, fetchWorkflowRuns,
    workflowVars, workflowSecretKeys, setWorkflowVar, deleteWorkflowVar,
    setWorkflowSecret, deleteWorkflowSecret, fetchWorkflowVars, fetchWorkflowSecretKeys,
    generateWorkflow, generateDialogOpen, generateDescription,
    setGenerateDialogOpen, setGenerateDescription,
  } = useStore();

  useEffect(() => {
    fetchWorkflows();
    fetchWorkflowRuns();
    fetchWorkflowVars();
    fetchWorkflowSecretKeys();
  }, [fetchWorkflows, fetchWorkflowRuns, fetchWorkflowVars, fetchWorkflowSecretKeys]);

  return (
    <ManagerPageLayout
      icon={<Workflow size={20} className="text-white" />}
      title="工作流管理"
      subtitle={`共 ${workflows.length} 个工作流`}
      headerActions={
        <button
          onClick={() => setGenerateDialogOpen(true)}
          className="flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white rounded-lg text-sm transition-all shadow-sm hover:shadow-md font-medium"
        >
          <Sparkles size={15} />
          AI 生成
        </button>
      }
    >
      <div className="max-w-3xl mx-auto space-y-3">
          {workflowResult && (
            <div className="p-3 bg-green-50 dark:bg-green-900/20 border border-green-100 dark:border-green-800 rounded-lg">
              <pre className="text-xs text-green-700 dark:text-green-400 whitespace-pre-wrap">{workflowResult}</pre>
              <button onClick={clearWorkflowResult} className="text-xs text-green-500 underline mt-1">关闭</button>
            </div>
          )}
          {workflowError && (
            <div className="p-3 bg-red-50 dark:bg-red-900/20 border border-red-100 dark:border-red-800 rounded-lg text-sm text-red-600 dark:text-red-400">
              {workflowError}
              <button onClick={clearWorkflowResult} className="ml-2 underline">关闭</button>
            </div>
          )}

          {workflows.length === 0 && !workflowLoading && (
            <div className="p-8 text-center text-gray-400 dark:text-gray-500">
              <Workflow size={32} className="mx-auto mb-2 opacity-50" />
              <p className="text-sm">暂无工作流</p>
              <p className="text-xs mt-1">在 ~/.config/agent/workflows/ 目录下创建 .yaml 文件来定义工作流，或使用 AI 生成</p>
            </div>
          )}

          {workflows.map((wf) => (
            <div key={wf.name} className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
              <div className="flex items-center gap-4">
                <div className="w-9 h-9 rounded-lg bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center flex-shrink-0">
                  <Workflow size={18} className="text-purple-600 dark:text-purple-400" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <h4 className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">{wf.name}</h4>
                    <span className="text-xs text-gray-400">{wf.step_count} 步骤</span>
                    {wf.trigger && (
                      <span className={`text-xs px-1.5 py-0.5 rounded-full ${
                        wf.trigger.startsWith('cron') ? 'bg-amber-100 text-amber-600' :
                        wf.trigger.startsWith('file_watch') ? 'bg-blue-100 text-blue-600' :
                        'bg-gray-200 text-gray-500'
                      }`}>
                        {wf.trigger.startsWith('cron') ? '⏰' : wf.trigger.startsWith('file_watch') ? '👁️' : '🖱️'}
                      </span>
                    )}
                    {wf.last_run_status && (
                      <span className={`text-xs px-1.5 py-0.5 rounded-full ${wf.last_run_status === 'completed' ? 'bg-green-100 text-green-600' : 'bg-red-100 text-red-600'}`}>{wf.last_run_status}</span>
                    )}
                  </div>
                  <p className="text-xs text-gray-500 mt-0.5 truncate">{wf.description || wf.file_path}</p>
                  {wf.next_run_at && <p className="text-xs text-amber-500 mt-1">下次执行: {wf.next_run_at}</p>}
                </div>
                <button onClick={() => runWorkflow(wf.name)} disabled={workflowLoading}
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white text-xs rounded-lg transition-all">
                  <Play size={12} /> 运行
                </button>
              </div>
            </div>
          ))}

          {/* Execution history */}
          {workflowRuns.length > 0 && (
            <div className="mt-4">
              <h4 className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2 flex items-center gap-1"><Clock size={12} /> 执行历史</h4>
              <div className="space-y-2">
                {workflowRuns.slice(0, 5).map((run) => (
                  <div key={run.id} className="rounded-lg bg-gray-50/50 dark:bg-gray-700/30 border border-gray-100 dark:border-gray-700 p-3">
                    <div className="flex items-center gap-2 text-xs">
                      <span className={`w-2 h-2 rounded-full ${run.status === 'completed' ? 'bg-green-500' : run.status === 'failed' ? 'bg-red-500' : 'bg-yellow-500 animate-pulse'}`} />
                      <span className="font-medium text-gray-700 dark:text-gray-300">{run.workflow_name}</span>
                      <span className="text-gray-400">{run.status}</span>
                      <span className="text-gray-400 ml-auto text-[10px]">{run.started_at}</span>
                    </div>
                    {run.step_progress && (() => {
                      try {
                        const steps = JSON.parse(run.step_progress);
                        return (
                          <div className="mt-2 ml-4 space-y-1">
                            {steps.map((step: any) => (
                              <div key={step.step_id} className="flex items-center gap-2 text-[10px]">
                                <span className={`w-1.5 h-1.5 rounded-full ${step.status === 'completed' ? 'bg-green-400' : step.status === 'failed' ? 'bg-red-400' : 'bg-gray-300'}`} />
                                <span className="text-gray-600 dark:text-gray-400">{step.step_id}</span>
                                <span className="text-gray-400">{step.duration_ms ? `${step.duration_ms}ms` : ''}</span>
                                {step.error && <span className="text-red-400 truncate max-w-[200px]">{step.error}</span>}
                              </div>
                            ))}
                          </div>
                        );
                      } catch { return null; }
                    })()}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Variables & Secrets */}
          <div className="mt-4">
            <h4 className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2">工作流变量</h4>
            <div className="space-y-1 mb-3">
              {Object.entries(workflowVars).map(([key, value]) => (
                <div key={key} className="flex items-center gap-2 px-3 py-1.5 rounded bg-gray-50/50 dark:bg-gray-700/30 text-xs">
                  <span className="font-mono text-gray-600 dark:text-gray-400">{key}</span>
                  <span className="text-gray-400">=</span>
                  <span className="text-gray-500 truncate flex-1">{value}</span>
                  <button onClick={() => deleteWorkflowVar(key)} className="text-red-400 hover:text-red-600">✕</button>
                </div>
              ))}
            </div>
            <AddVarForm onAdd={(k, v) => setWorkflowVar(k, v)} />

            <h4 className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2 mt-3">工作流密钥</h4>
            <div className="space-y-1 mb-3">
              {workflowSecretKeys.map((key) => (
                <div key={key} className="flex items-center gap-2 px-3 py-1.5 rounded bg-gray-50/50 dark:bg-gray-700/30 text-xs">
                  <span className="font-mono text-gray-600 dark:text-gray-400">{key}</span>
                  <span className="text-gray-400">= ******</span>
                  <button onClick={() => deleteWorkflowSecret(key)} className="text-red-400 hover:text-red-600 ml-auto">✕</button>
                </div>
              ))}
            </div>
            <AddSecretForm onAdd={(k, v) => setWorkflowSecret(k, v)} />
          </div>

          {/* AI Generate Dialog */}
          {generateDialogOpen && (
            <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
              <div className="bg-white dark:bg-gray-800 rounded-2xl w-[520px] shadow-2xl animate-in fade-in zoom-in-95 duration-200">
                <div className="flex items-center justify-between p-5 border-b border-gray-100 dark:border-gray-700">
                  <div className="flex items-center gap-2">
                    <div className="w-8 h-8 rounded-lg bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center">
                      <Sparkles size={16} className="text-purple-600 dark:text-purple-400" />
                    </div>
                    <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">AI 工作流生成器</h2>
                  </div>
                  <button
                    onClick={() => setGenerateDialogOpen(false)}
                    className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                  >
                    <X size={18} />
                  </button>
                </div>

                <div className="p-5 space-y-4">
                  <div>
                    <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">
                      描述你想要的工作流
                    </label>
                    <textarea
                      value={generateDescription}
                      onChange={e => setGenerateDescription(e.target.value)}
                      placeholder="例如：每天早上 9 点扫描 downloads 目录，将新增的 PDF 文件归类到对应子目录..."
                      rows={4}
                      className="w-full bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500 resize-none"
                    />
                  </div>

                  {workflowError && (
                    <div className="p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-700 dark:text-red-400 flex items-start gap-2">
                      <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" />
                      <span>{workflowError}</span>
                    </div>
                  )}

                  <div className="flex gap-2 pt-2">
                    <button
                      onClick={() => generateWorkflow(generateDescription)}
                      disabled={!generateDescription.trim() || workflowLoading}
                      className="flex-1 flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2.5 rounded-lg text-sm transition-all font-medium"
                    >
                      {workflowLoading ? (
                        <><Loader2 size={14} className="animate-spin" /> 生成中...</>
                      ) : (
                        <><Sparkles size={14} /> 生成工作流</>
                      )}
                    </button>
                    <button
                      onClick={() => setGenerateDialogOpen(false)}
                      className="px-4 py-2.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-colors"
                    >
                      取消
                    </button>
                  </div>
                </div>
              </div>
            </div>
          )}

          {workflowLoading && (
            <div className="flex items-center justify-center py-4 text-gray-400">
              <Loader2 size={16} className="animate-spin mr-2" />
              <span className="text-sm">运行中...</span>
            </div>
          )}
        </div>
    </ManagerPageLayout>
  );
}

function AddVarForm({ onAdd }: { onAdd: (k: string, v: string) => void }) {
  const [k, setK] = useState('');
  const [v, setV] = useState('');
  const handleAdd = () => { if (k.trim()) { onAdd(k.trim(), v); setK(''); setV(''); } };
  return (
    <div className="flex gap-2">
      <input value={k} onChange={e => setK(e.target.value)} placeholder="变量名" className="flex-1 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-2 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400" />
      <input value={v} onChange={e => setV(e.target.value)} placeholder="值" className="flex-1 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-2 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400" />
      <button onClick={handleAdd} className="px-3 py-1.5 bg-purple-600 hover:bg-purple-700 text-white text-xs rounded-lg">添加</button>
    </div>
  );
}

function AddSecretForm({ onAdd }: { onAdd: (k: string, v: string) => void }) {
  const [k, setK] = useState('');
  const [v, setV] = useState('');
  const handleAdd = () => { if (k.trim()) { onAdd(k.trim(), v); setK(''); setV(''); } };
  return (
    <div className="flex gap-2">
      <input value={k} onChange={e => setK(e.target.value)} placeholder="密钥名" className="flex-1 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-2 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400" />
      <input value={v} onChange={e => setV(e.target.value)} type="password" placeholder="密钥值" className="flex-1 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-2 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400" />
      <button onClick={handleAdd} className="px-3 py-1.5 bg-purple-600 hover:bg-purple-700 text-white text-xs rounded-lg">添加</button>
    </div>
  );
}
