/// Lightweight in-memory vector index with cosine similarity search.
///
/// Stores `(id, text, embedding)` triples and supports top-k retrieval.
/// The index is rebuilt from SQLite at application startup and is NOT
/// persisted independently — SQLite is the source of truth.
use std::collections::HashMap;

#[derive(Clone)]
struct IndexEntry {
    text: String,
    embedding: Vec<f32>,
}

/// A simple in-memory vector index.
pub struct InMemoryVectorIndex {
    entries: HashMap<String, IndexEntry>,
}

impl InMemoryVectorIndex {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Insert or update an entry.
    pub fn insert(&mut self, id: String, text: String, embedding: Vec<f32>) {
        self.entries.insert(id, IndexEntry { text, embedding });
    }

    /// Remove an entry by ID.
    pub fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }

    /// Search for the top-k most similar entries.
    /// Returns `(id, score)` pairs sorted by descending similarity.
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

        // Sort by descending similarity
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(k);
        scores
    }

    /// Total number of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Compute cosine similarity between two vectors.
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
        // Normalize to unit vector for consistent cosine similarity
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
        assert_eq!(results[0].0, "a"); // most similar
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
        let results = index.search(&vec![1.0, 0.0], 5);
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
