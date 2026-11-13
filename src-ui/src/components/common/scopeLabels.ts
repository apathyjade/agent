/**
 * 资源作用域状态标签 → 中文说明。
 *
 * 后端 `ScopeStatus::label()` 是稳定英文标识；这里集中做展示映射，
 * 未知状态原样返回，避免静默丢信息。
 */
export function scopeStatusLabel(status: string): string {
  switch (status) {
    case 'applied':
      return '已按请求收窄';
    case 'not_requested':
      return '未启用收窄';
    case 'skipped_no_catalog':
      return '无资源目录，未收窄';
    case 'skipped_small_catalog':
      return '候选资源太少，未收窄';
    case 'skipped_no_budget':
      return '路由预算不足，未收窄';
    case 'skipped_unavailable':
      return '收窄服务不可用，未收窄';
    default:
      return status;
  }
}
