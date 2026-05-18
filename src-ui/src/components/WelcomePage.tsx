import { useState } from 'react';
import { Sparkles, Search, Code, PenTool, Image, Globe } from 'lucide-react';
import { useStore } from '../store';

export function WelcomePage() {
  const [input, setInput] = useState('');
  const createConversation = useStore((state) => state.createConversation);
  const sendMessageStream = useStore((state) => state.sendMessageStream);
  const defaultModel = useStore((state) => state.defaultModel);

  const handleQuickStart = async (prompt: string) => {
    const modelId = defaultModel || '';
    if (!modelId) return;
    await createConversation('New Chat', modelId);
    setTimeout(() => sendMessageStream(prompt), 100);
  };

  const features = [
    { icon: <Search size={20} />, title: '深入研究', desc: '深度分析复杂主题' },
    { icon: <Code size={20} />, title: '网页开发', desc: '生成完整网页代码' },
    { icon: <PenTool size={20} />, title: '内容创作', desc: '文章、文案创作' },
    { icon: <Image size={20} />, title: '图像生成', desc: 'AI 绘画创作' },
    { icon: <Globe size={20} />, title: '联网搜索', desc: '实时获取最新信息' },
    { icon: <Sparkles size={20} />, title: '深度思考', desc: '复杂问题推理' },
  ];

  return (
    <div className="flex-1 flex flex-col items-center justify-center p-8 bg-gradient-to-b from-purple-50/50 to-white">
      <div className="max-w-2xl w-full text-center">
        <div className="mb-8">
          <div className="w-16 h-16 mx-auto mb-4 rounded-2xl bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center shadow-lg">
            <Sparkles size={32} className="text-white" />
          </div>
          <h1 className="text-3xl font-bold text-gray-900 mb-2">你好，我是 Agent</h1>
          <p className="text-gray-500">你的 AI 智能助手，随时为你提供帮助</p>
        </div>

        <div className="relative mb-8">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && input.trim()) {
                e.preventDefault();
                handleQuickStart(input);
              }
            }}
            placeholder="有什么可以帮你的？"
            className="w-full bg-white border border-gray-200 rounded-2xl px-6 py-4 pr-14 text-base resize-none focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent shadow-sm min-h-[60px] max-h-[200px]"
            rows={1}
          />
          <button
            onClick={() => input.trim() && handleQuickStart(input)}
            disabled={!input.trim()}
            className="absolute right-3 bottom-3 p-2 rounded-xl bg-gradient-to-r from-purple-600 to-indigo-600 text-white disabled:opacity-30 disabled:cursor-not-allowed hover:shadow-lg transition-all"
          >
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 12h14M12 5l7 7-7 7" />
            </svg>
          </button>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
          {features.map((feature) => (
            <button
              key={feature.title}
              onClick={() => handleQuickStart(feature.desc)}
              className="flex items-center gap-3 p-4 bg-white border border-gray-100 rounded-xl hover:border-purple-200 hover:shadow-md transition-all text-left group"
            >
              <div className="text-gray-400 group-hover:text-purple-600 transition-colors">
                {feature.icon}
              </div>
              <div>
                <h3 className="text-sm font-medium text-gray-900">{feature.title}</h3>
                <p className="text-xs text-gray-500">{feature.desc}</p>
              </div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
