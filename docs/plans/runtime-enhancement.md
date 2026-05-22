# 运行时管理增强计划

## Sprint 1 — 体验修复
1.1+1.2 RuntimeCard: 可用版本展开收起 + LifecycleBadge + 日期/大小
1.3 RuntimeManagerPage: 安装进度阻塞弹窗→浮动浮标
1.4 ProjectBindingPanel: 一键补齐缺失运行时

## Sprint 2 — 核心功能
2.2 HealthCenter: 选择性升级+大版本预警
2.4 VersionCompareDialog + RuntimeCard对比checkbox
2.3 系统检测tab: PATH冲突可操作化
2.1 后端磁盘统计API + HealthCenter磁盘占用展示

## Sprint 3 — 进阶功能
3.1 全局搜索栏
3.2 骨架屏
3.3 通知集成

## Sprint 4 — 锦上添花
4.1 自检报告
4.5 快捷键

## 用户反馈调整
- 可用版本移入VersionSelector弹窗，RuntimeCard只显示已安装版本
- 进度浮标移到右下角，调大尺寸
- VersionSelector弹窗调大
- 点击安装后关闭弹窗
- 移除RuntimeCard无用箭头
- 错误toast化

## Python版本源修复
- tag: 20250115→20260510
- 版本号更新
- Windows ext: zip→tar.gz

## 版本管理工具集成
- manager_detector.rs: fnm/nvm/uv/rustup检测
- manager_executor.rs: 命令执行
- IPC命令 + frontend选择器UI + 持久化
- Phase3: 管理工具自动安装
