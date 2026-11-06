//! 轻量内存向量索引，支持余弦相似度检索。
//!
//! 保存 `(id, text, embedding)` 三元组并支持 top-k 召回。索引在应用启动后
//! 从 SQLite 懒加载重建，不单独持久化——SQLite 才是唯一事实来源。

use std::collections::HashMap;

#[derive(Clone)]
struct IndexEntry {
    /// 与 embedding 一起保留，便于排查与重建。
    #[allow(dead_code)]
    text: String,
    embedding: Vec<f32>,
}

/// 简单的内存向量索引。
pub struct InMemoryVectorIndex {
    entries: HashMap<String, IndexEntry>,
}

impl Default for InMemoryVectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryVectorIndex {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// 插入或更新一条记录。
    pub fn insert(&mut self, id: String, text: String, embedding: Vec<f32>) {
        self.entries.insert(id, IndexEntry { text, embedding });
    }

    /// 按 id 删除记录。
    pub fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }

    /// 检索相似度最高的 top-k 条记录。
    /// 返回 `(id, score)`，按相似度降序排列。
    pub fn search(&self, query_embedding: &[f32], k: usize) -> Vec<(String, f32)> {
        let mut scores: Vec<(String, f32)> = self
            .entries
            .iter()
            .map(|(id, entry)| {
                let sim = cosine_similarity(&entry.embedding, query_embedding);
                (id.clone(), sim)
            })
            .filter(|(_, sim)| sim.is_finite())
            .collect();

        // 按相似度降序排序
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(k);
        scores
    }

    /// 索引中的记录总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 索引是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 计算两个向量的余弦相似度。
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_embedding(values: Vec<f32>) -> Vec<f32> {
        // 归一化为单位向量，使余弦相似度结果稳定
        let norm: f32 = values.iter().map(|x| x * x).sum::<f32>().sqrt();
        values.into_iter().map(|x| x / norm).collect()
    }

    #[test]
    fn test_insert_and_search() {
        let mut index = InMemoryVectorIndex::new();
        index.insert("a".into(), "hello world".into(), make_embedding(vec![1.0, 0.0]));
        index.insert("b".into(), "goodbye world".into(), make_embedding(vec![0.0, 1.0]));

        let results = index.search(&make_embedding(vec![1.0, 0.1]), 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "a"); // 最相似
        assert_eq!(results[1].0, "b");
    }

    #[test]
    fn test_search_limit() {
        let mut index = InMemoryVectorIndex::new();
        for i in 0..10 {
            let v = vec![i as f32, 0.0];
            index.insert(
                format!("id_{}", i),
                format!("item {}", i),
                make_embedding(v),
            );
        }

        let results = index.search(&make_embedding(vec![1.0, 0.0]), 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_empty_index() {
        let index = InMemoryVectorIndex::new();
        let results = index.search(&[1.0, 0.0], 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_remove() {
        let mut index = InMemoryVectorIndex::new();
        index.insert("a".into(), "hello".into(), make_embedding(vec![1.0, 0.0]));
        index.remove("a");
        assert!(index.is_empty());
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = make_embedding(vec![3.0, 4.0]);
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = make_embedding(vec![1.0, 0.0]);
        let b = make_embedding(vec![0.0, 1.0]);
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[]), 0.0);
    }
}
