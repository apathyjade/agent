//! 进程内意图向量索引。
//!
//! 每个意图有一个 `name + description` 向量，外加每条 example 一个向量。意图
//! 分数取其所有单元上的最大 cosine。索引在后台重建；替换完成前读取方继续使用
//! 旧快照。

use std::collections::{HashMap, HashSet};

/// 单个意图的 embedding 单元。
#[derive(Debug, Clone, Default)]
struct IntentUnits {
    vectors: Vec<Vec<f32>>,
}

/// 可整体替换快照的意图索引。
#[derive(Debug, Default)]
pub struct IntentVectorIndex {
    units: HashMap<String, IntentUnits>,
    ready: bool,
    dirty: bool,
}

impl IntentVectorIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 整体替换快照并标记为就绪。
    pub fn replace(&mut self, units: HashMap<String, Vec<Vec<f32>>>) {
        self.units = units
            .into_iter()
            .map(|(intent_id, vectors)| (intent_id, IntentUnits { vectors }))
            .collect();
        self.ready = true;
        self.dirty = false;
    }

    /// 丢弃快照（例如没有可用的 embedder 时）。
    pub fn clear(&mut self) {
        self.units.clear();
        self.ready = false;
        self.dirty = false;
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// 对所有候选意图与 `query` 打分，返回 top `k`。
    ///
    /// 分数是原始 cosine 相似度。没有向量的候选会被跳过；同分时按意图 id 排序，
    /// 保证顺序稳定。
    pub fn top_k(
        &self,
        query: &[f32],
        candidates: &HashSet<String>,
        k: usize,
    ) -> Vec<(String, f32)> {
        if !self.ready || query.is_empty() || k == 0 {
            return Vec::new();
        }

        let mut scored: Vec<(String, f32)> = candidates
            .iter()
            .filter_map(|intent_id| {
                let units = self.units.get(intent_id)?;
                let best = units
                    .vectors
                    .iter()
                    .map(|vector| cosine(query, vector))
                    .fold(0.0f32, f32::max);
                Some((intent_id.clone(), best))
            })
            .collect();

        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(k);
        scored
    }
}

/// cosine 相似度；维度不一致或零向量返回 `0.0`。
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (left, right) in a.iter().zip(b.iter()) {
        dot += left * right;
        norm_a += left * left;
        norm_b += right * right;
    }

    if norm_a <= f32::EPSILON || norm_b <= f32::EPSILON {
        return 0.0;
    }

    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// 构造单个意图需要 embedding 的文本单元。
pub fn intent_units(name: &str, description: &str, examples: &[String]) -> Vec<String> {
    let mut units = Vec::with_capacity(1 + examples.len());
    let profile = format!("{name}\n{description}").trim().to_string();
    if !profile.is_empty() {
        units.push(profile);
    }
    for example in examples {
        let trimmed = example.trim();
        if !trimmed.is_empty() {
            units.push(trimmed.to_string());
        }
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn cosine_is_one_for_identical_vectors() {
        assert!((cosine(&[1.0, 2.0], &[1.0, 2.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_is_zero_for_orthogonal_vectors() {
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn cosine_rejects_mismatched_dimensions() {
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn top_k_uses_maximum_unit_score() {
        let mut index = IntentVectorIndex::new();
        index.replace(HashMap::from([
            ("a".to_string(), vec![vec![1.0, 0.0], vec![0.0, 1.0]]),
            ("b".to_string(), vec![vec![0.0, 1.0]]),
            ("c".to_string(), vec![vec![-1.0, 0.0]]),
        ]));

        let scored = index.top_k(&[1.0, 0.0], &candidate_set(&["a", "b"]), 2);

        assert_eq!(scored[0], ("a".to_string(), 1.0));
        assert_eq!(scored[1].0, "b");
        assert!(scored[1].1.abs() < 1e-6);
    }

    #[test]
    fn top_k_skips_intents_without_vectors() {
        let mut index = IntentVectorIndex::new();
        index.replace(HashMap::from([("a".to_string(), vec![vec![1.0, 0.0]])]));

        let scored = index.top_k(&[1.0, 0.0], &candidate_set(&["a", "missing"]), 5);

        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].0, "a");
    }

    #[test]
    fn not_ready_index_returns_nothing() {
        let index = IntentVectorIndex::new();
        assert!(index.top_k(&[1.0, 0.0], &candidate_set(&["a"]), 1).is_empty());
    }

    #[test]
    fn replace_clears_dirty_and_sets_ready() {
        let mut index = IntentVectorIndex::new();
        assert!(!index.is_ready());
        index.mark_dirty();
        assert!(index.is_dirty());

        index.replace(HashMap::from([("a".to_string(), vec![vec![1.0]])]));

        assert!(index.is_ready());
        assert!(!index.is_dirty());
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn intent_units_include_profile_and_examples() {
        let units = intent_units("实现功能", "写代码", &["加个开关".to_string()]);
        assert_eq!(units.len(), 2);
        assert!(units[0].contains("实现功能"));
        assert_eq!(units[1], "加个开关");
    }

    #[test]
    fn intent_units_skip_blank_examples() {
        let units = intent_units("", "", &["".to_string(), "  ".to_string()]);
        assert!(units.is_empty());
    }
}
