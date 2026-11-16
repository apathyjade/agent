//! 资源使用反馈：记录「收窄给了什么」与「实际用了什么」，给出收缩建议。
//!
//! 纯逻辑、无 IO：输入是运行期样本，输出是可序列化的建议列表。
//! 判定口径保守——只有在足够多样本下从未被使用的工具才建议移出。

use std::collections::BTreeMap;

use serde::Serialize;

/// 给出「建议移出」所需的最少样本数。
pub const MIN_SAMPLES_FOR_SUGGESTION: usize = 3;

/// 一次执行的资源使用样本：收窄后的可见工具与实际调用过的工具。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSample {
    pub scoped: Vec<String>,
    pub used: Vec<String>,
}

/// 单个工具的累计使用情况。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolUsage {
    pub tool: String,
    /// 被收窄纳入可见集合的次数。
    pub scoped_times: usize,
    /// 实际被调用的次数。
    pub used_times: usize,
    /// 是否建议移出：样本足够且从未被使用。
    pub suggested_removal: bool,
}

/// 聚合使用样本，按「纳入次数降序、工具名升序」排序。
///
/// 只统计样本里出现过的工具；`used` 中不属于 `scoped` 的条目会被忽略
/// （越界调用由运行时守卫负责，不进入收缩建议）。
pub fn aggregate(samples: &[UsageSample]) -> Vec<ToolUsage> {
    let mut scoped: BTreeMap<String, usize> = BTreeMap::new();
    let mut used: BTreeMap<String, usize> = BTreeMap::new();

    for sample in samples {
        for tool in &sample.scoped {
            *scoped.entry(tool.clone()).or_insert(0) += 1;
        }
        for tool in &sample.used {
            if sample.scoped.iter().any(|scoped_tool| scoped_tool == tool) {
                *used.entry(tool.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut rows: Vec<ToolUsage> = scoped
        .into_iter()
        .map(|(tool, scoped_times)| {
            let used_times = used.get(&tool).copied().unwrap_or(0);
            let suggested_removal =
                scoped_times >= MIN_SAMPLES_FOR_SUGGESTION && used_times == 0;
            ToolUsage {
                tool,
                scoped_times,
                used_times,
                suggested_removal,
            }
        })
        .collect();

    rows.sort_by(|a, b| {
        b.scoped_times
            .cmp(&a.scoped_times)
            .then_with(|| a.tool.cmp(&b.tool))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(scoped: &[&str], used: &[&str]) -> UsageSample {
        UsageSample {
            scoped: scoped.iter().map(|tool| tool.to_string()).collect(),
            used: used.iter().map(|tool| tool.to_string()).collect(),
        }
    }

    #[test]
    fn empty_samples_produce_no_rows() {
        assert!(aggregate(&[]).is_empty());
    }

    #[test]
    fn counts_scoped_and_used_per_tool() {
        let samples = vec![
            sample(&["read", "shell"], &["read"]),
            sample(&["read", "shell"], &["shell"]),
        ];

        let rows = aggregate(&samples);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].tool, "read");
        assert_eq!(rows[0].scoped_times, 2);
        assert_eq!(rows[0].used_times, 1);
        assert!(!rows[0].suggested_removal);
        assert_eq!(rows[1].tool, "shell");
        assert_eq!(rows[1].used_times, 1);
    }

    #[test]
    fn suggests_removal_only_after_enough_unused_samples() {
        let samples = vec![
            sample(&["read", "shell"], &["read"]),
            sample(&["read", "shell"], &["read"]),
        ];
        // 只有两个样本：还没到阈值。
        assert!(aggregate(&samples).iter().all(|row| !row.suggested_removal));

        let mut enough = samples;
        enough.push(sample(&["read", "shell"], &["read"]));

        let shell = aggregate(&enough)
            .into_iter()
            .find(|row| row.tool == "shell")
            .expect("shell 应出现在聚合结果里");
        assert_eq!(shell.scoped_times, 3);
        assert_eq!(shell.used_times, 0);
        assert!(shell.suggested_removal);
    }

    #[test]
    fn ignores_used_tools_outside_the_scoped_set() {
        let samples = vec![sample(&["read"], &["shell"])];

        let rows = aggregate(&samples);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tool, "read");
        assert_eq!(rows[0].used_times, 0);
    }

    #[test]
    fn sorts_by_scope_frequency_then_name() {
        let samples = vec![sample(&["b", "a", "c"], &[])];

        let tools: Vec<String> = aggregate(&samples).into_iter().map(|row| row.tool).collect();

        assert_eq!(tools, vec!["a", "b", "c"]);
    }
}
