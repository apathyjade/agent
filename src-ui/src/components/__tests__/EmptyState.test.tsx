import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { EmptyState } from '../EmptyState';

describe('EmptyState', () => {
  it('renders conversations empty state', () => {
    render(<EmptyState type="conversations" />);
    expect(screen.getByText('暂无对话')).toBeDefined();
    expect(screen.getByText('点击"新对话"按钮开始第一个对话')).toBeDefined();
  });

  it('renders messages empty state', () => {
    render(<EmptyState type="messages" />);
    expect(screen.getByText('开始对话')).toBeDefined();
    expect(screen.getByText('发送一条消息开始与 AI 对话')).toBeDefined();
  });

  it('renders search empty state', () => {
    render(<EmptyState type="search" />);
    expect(screen.getByText('无搜索结果')).toBeDefined();
    expect(screen.getByText('尝试其他搜索词')).toBeDefined();
  });
});
