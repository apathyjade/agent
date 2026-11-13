//! 收窄指标聚合：把进程内最近的 [`RouteTrace`] 汇总成一组可展示的比率。
//!
//! 纯逻辑、无 IO：输入是轨迹切片，输出是可直接序列化的指标快照。
//! 统计口径与 [`crate::llm::intent::ScopeStatus`] 一致：
//! `scope` 为 `None` 视为未请求收窄，`skipped` 非空视为跳过。

use serde::Serialize;

use super::RouteTrace;

/// 最近轨迹的收窄汇总。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScopeMetrics {
    /// 参与统计的轨迹条数。
    pub samples: usize,
    /// 真正进入收窄的次数（命中缓存也算尝试）。
    pub attempted: usize,
    /// 收窄完成且未命中缓存的次数。
    pub applied: usize,
    /// 收窄完成且命中缓存的次数。
    pub cached: usize,
    /// 因目录缺失、候选太少、预算不足或服务不可用而跳过的次数。
    pub skipped: usize,
    /// 未请求收窄的次数（零意图、开关关闭或旧数据）。
    pub not_requested: usize,
    /// 收窄成功样本的选中资源总数。
    pub selected_total: usize,
    /// 收窄成功样本的目录规模总数。
    pub catalog_total: usize,
    /// 全部样本的平均路由耗时（毫秒）。
    pub avg_elapsed_ms: f64,
    /// 全部样本的平均 LLM 调用次数。
    pub avg_llm_calls: f64,
    /// 收窄成功率 = (applied + cached) / attempted。
    pub applied_rate: f64,
    /// 缓存命中率 = cached / attempted。
    pub cache_rate: f64,
    /// 跳过率 = skipped / samples。
    pub skip_rate: f64,
    /// 平均选中比 = selected_total / catalog_total（仅收窄成功的样本）。
    pub selected_ratio: f64,
}

impl ScopeMetrics {
    /// 空样本：所有计数为 0，比率也为 0，避免前端出现 `NaN`。
    pub fn empty() -> Self {
        Self {
            samples: 0,
            attempted: 0,
            applied: 0,
            cached: 0,
            skipped: 0,
            not_requested: 0,
            selected_total: 0,
            catalog_total: 0,
            avg_elapsed_ms: 0.0,
            avg_llm_calls: 0.0,
            applied_rate: 0.0,
            cache_rate: 0.0,
            skip_rate: 0.0,
            selected_ratio: 0.0,
        }
    }

    /// 从轨迹切片聚合指标。样本顺序无关，结果确定。
    pub fn from_traces(traces: &[RouteTrace]) -> Self {
        if traces.is_empty() {
            return Self::empty();
        }

        let mut metrics = Self {
            samples: traces.len(),
            ..Self::empty()
        };
        let mut elapsed_total: u64 = 0;
        let mut llm_calls_total: usize = 0;

        for trace in traces {
            elapsed_total += trace.elapsed_ms;
            llm_calls_total += trace.llm_calls;

            match &trace.scope {
                None => metrics.not_requested += 1,
                Some(scope) => match &scope.skipped {
                    Some(_) => metrics.skipped += 1,
                    None => {
                        metrics.attempted += 1;
                        if scope.cached {
                            metrics.cached += 1;
                        } else {
                            metrics.applied += 1;
                        }
                        metrics.selected_total += scope.selected;
                        metrics.catalog_total += scope.catalog_size;
                    }
                },
            }
        }

        let samples = metrics.samples as f64;
        metrics.avg_elapsed_ms = elapsed_total as f64 / samples;
        metrics.avg_llm_calls = llm_calls_total as f64 / samples;

        if metrics.attempted > 0 {
            let attempted = metrics.attempted as f64;
            metrics.applied_rate = (metrics.applied + metrics.cached) as f64 / attempted;
            metrics.cache_rate = metrics.cached as f64 / attempted;
        }
        metrics.skip_rate = metrics.skipped as f64 / samples;
        if metrics.catalog_total > 0 {
            metrics.selected_ratio = metrics.selected_total as f64 / metrics.catalog_total as f64;
        }

        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::intent::{DecisionSource, ScopeTrace};

    fn trace(scope: Option<ScopeTrace>, elapsed_ms: u64, llm_calls: usize) -> RouteTrace {
        RouteTrace {
            at: "2026-11-14T00:00:00Z".to_string(),
            session_id: "session-1".to_string(),
            message_excerpt: "导出 CSV".to_string(),
            intents: Vec::new(),
            candidates: Vec::new(),
            confidence: 0.0,
            uncertain: false,
            source: DecisionSource::Llm,
            elapsed_ms,
            llm_calls,
            probe_used: false,
            scope,
        }
    }

    fn applied(selected: usize, catalog_size: usize, cached: bool) -> Option<ScopeTrace> {
        Some(ScopeTrace {
            catalog_size,
            selected,
            cached,
            skipped: None,
        })
    }

    fn skipped(reason: &str) -> Option<ScopeTrace> {
        Some(ScopeTrace {
            catalog_size: 0,
            selected: 0,
            cached: false,
            skipped: Some(reason.to_string()),
        })
    }

    #[test]
    fn empty_samples_produce_zero_metrics() {
        let metrics = ScopeMetrics::from_traces(&[]);

        assert_eq!(metrics, ScopeMetrics::empty());
        assert_eq!(metrics.applied_rate, 0.0);
        assert_eq!(metrics.selected_ratio, 0.0);
    }

    #[test]
    fn applied_and_cached_samples_accumulate_selection() {
        let traces = vec![
            trace(applied(4, 10, false), 100, 2),
            trace(applied(2, 10, true), 200, 3),
        ];

        let metrics = ScopeMetrics::from_traces(&traces);

        assert_eq!(metrics.samples, 2);
        assert_eq!(metrics.attempted, 2);
        assert_eq!(metrics.applied, 1);
        assert_eq!(metrics.cached, 1);
        assert_eq!(metrics.selected_total, 6);
        assert_eq!(metrics.catalog_total, 20);
        assert_eq!(metrics.applied_rate, 1.0);
        assert_eq!(metrics.cache_rate, 0.5);
        assert_eq!(metrics.selected_ratio, 0.3);
        assert_eq!(metrics.avg_elapsed_ms, 150.0);
        assert_eq!(metrics.avg_llm_calls, 2.5);
    }

    #[test]
    fn skipped_and_not_requested_are_counted_separately() {
        let traces = vec![
            trace(skipped("skipped_small_catalog"), 10, 1),
            trace(skipped("skipped_no_budget"), 10, 1),
            trace(None, 20, 1),
        ];

        let metrics = ScopeMetrics::from_traces(&traces);

        assert_eq!(metrics.samples, 3);
        assert_eq!(metrics.attempted, 0);
        assert_eq!(metrics.skipped, 2);
        assert_eq!(metrics.not_requested, 1);
        assert_eq!(metrics.skip_rate, 2.0 / 3.0);
        // 没有收窄样本时，比率保持 0 而不是 NaN。
        assert_eq!(metrics.applied_rate, 0.0);
        assert_eq!(metrics.selected_ratio, 0.0);
    }

    #[test]
    fn mixed_samples_report_each_bucket() {
        let traces = vec![
            trace(applied(3, 12, false), 50, 2),
            trace(skipped("skipped_no_catalog"), 60, 1),
            trace(None, 70, 0),
        ];

        let metrics = ScopeMetrics::from_traces(&traces);

        assert_eq!(metrics.attempted, 1);
        assert_eq!(metrics.applied, 1);
        assert_eq!(metrics.skipped, 1);
        assert_eq!(metrics.not_requested, 1);
        assert_eq!(metrics.selected_ratio, 0.25);
        assert_eq!(metrics.skip_rate, 1.0 / 3.0);
        assert_eq!(metrics.avg_elapsed_ms, 60.0);
    }
}
