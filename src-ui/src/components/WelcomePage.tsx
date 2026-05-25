import { useState } from 'react';
import { Sparkles, Search, Code, PenTool, Image, Globe } from 'lucide-react';
import { Row } from '@jelper/component';
import { useStore } from '../store';
import { ChatInput } from './ChatInput';
import { SessionConfigPanel } from './SessionConfigPanel';

export function WelcomePage() {
  const createSession = useStore((state) => state.createSession);
  const sendMessageStream = useStore((state) => state.sendMessageStream);
  const defaultModel = useStore((state) => state.defaultModel);

  const [showConfig, setShowConfig] = useState(false);
  const [pendingPrompt, setPendingPrompt] = useState<string | null>(null);

  const handleQuickStart = async (prompt: string) => {
    const modelId = defaultModel || '';
    if (!modelId) return;
    // Show config panel first
    setPendingPrompt(prompt);
    setShowConfig(true);
  };

  const handleConfigStart = async (config: { modelId: string; personaId: string | null; enabledTools: string[] }) => {
    setShowConfig(false);
    await createSession('New Chat', config.modelId, undefined, config.personaId ?? undefined);

    // Save tool config to the newly created session
    const state = useStore.getState();
    const newSessionId = state.currentSession?.id;
    if (newSessionId && config.enabledTools.length > 0) {
      await state.updateSessionConfig(newSessionId, { enabled_tools: config.enabledTools });
    }

    if (pendingPrompt) {
      setTimeout(() => sendMessageStream(pendingPrompt), 100);
      setPendingPrompt(null);
    }
  };

  const handleConfigCancel = () => {
    setShowConfig(false);
    setPendingPrompt(null);
  };

  if (showConfig) {
    return <SessionConfigPanel onStart={handleConfigStart} onCancel={handleConfigCancel} />;
  }

  const features = [
    { icon: <Search size={20} />, title: '深入研究', desc: '深度分析复杂主题' },
    { icon: <Code size={20} />, title: '网页开发', desc: '生成完整网页代码' },
    { icon: <PenTool size={20} />, title: '内容创作', desc: '文章、文案创作' },
    { icon: <Image size={20} />, title: '图像生成', desc: 'AI 绘画创作' },
    { icon: <Globe size={20} />, title: '联网搜索', desc: '实时获取最新信息' },
    { icon: <Sparkles size={20} />, title: '深度思考', desc: '复杂问题推理' },
  ];

  return (
    <div className="flex flex-col items-center justify-center p-8 bg-gradient-to-b from-purple-50/50 to-white dark:from-gray-800/50 dark:to-gray-900 h-full">
      <div className="max-w-2xl w-full text-center">
        <div className="mb-8">
          <div className="w-16 h-16 mx-auto mb-4 rounded-2xl bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center shadow-lg">
            <Sparkles size={32} className="text-white" />
          </div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-gray-100 mb-2">你好，我是 Agent</h1>
          <p className="text-gray-500 dark:text-gray-400">你的 AI 智能助手，随时为你提供帮助</p>
        </div>

        <div className="mb-8">
          <ChatInput
            onSend={handleQuickStart}
            placeholder="有什么可以帮你的？"
          />
        </div>

        <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
          {features.map((feature) => (
            <button
              key={feature.title}
              onClick={() => handleQuickStart(feature.desc)}
              className="p-4 bg-white dark:bg-gray-800 border border-gray-100 dark:border-gray-700 rounded-xl hover:border-purple-200 dark:hover:border-purple-700 hover:shadow-md transition-all text-left group"
            >
              <Row $align="center" $gap={12}>
                <Row.Item $fixed>
                  <div className="text-gray-400 dark:text-gray-500 group-hover:text-purple-600 dark:group-hover:text-purple-400 transition-colors">
                    {feature.icon}
                  </div>
                </Row.Item>
                <Row.Item $scale={1}>
                  <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">{feature.title}</h3>
                  <p className="text-xs text-gray-500 dark:text-gray-400">{feature.desc}</p>
                </Row.Item>
              </Row>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
