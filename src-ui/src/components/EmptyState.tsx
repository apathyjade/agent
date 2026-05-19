import { MessageSquare, Inbox } from 'lucide-react';

interface EmptyStateProps {
  type: 'conversations' | 'messages' | 'search';
}

const config = {
  conversations: {
    icon: Inbox,
    title: '暂无对话',
    description: '点击"新对话"按钮开始第一个对话',
  },
  messages: {
    icon: MessageSquare,
    title: '开始对话',
    description: '发送一条消息开始与 AI 对话',
  },
  search: {
    icon: MessageSquare,
    title: '无搜索结果',
    description: '尝试其他搜索词',
  },
};

export function EmptyState({ type }: EmptyStateProps) {
  const { icon: Icon, title, description } = config[type];

  return (
    <div className="flex flex-col items-center justify-center h-full text-center px-6">
      <div className="w-16 h-16 rounded-2xl bg-gray-100 dark:bg-gray-800 flex items-center justify-center mb-4">
        <Icon size={28} className="text-gray-400 dark:text-gray-500" />
      </div>
      <h3 className="text-base font-medium text-gray-900 dark:text-gray-100 mb-1">
        {title}
      </h3>
      <p className="text-sm text-gray-500 dark:text-gray-400 max-w-xs">
        {description}
      </p>
    </div>
  );
}
