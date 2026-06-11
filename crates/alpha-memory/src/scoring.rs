//! Scoring functions for memory retrieval ranking.
//!
//! These functions compute composite scores that determine which memories
//! are most relevant to a query. The composite score combines:
//! - **Similarity**: cosine similarity between query and memory embeddings
//! - **Importance**: the stored importance weight of the memory
//! - **Recency**: exponential decay based on time since last access

use alpha_common::types::Timestamp;

/// Compute cosine similarity between two vectors.
///
/// Returns a value in `[-1.0, 1.0]`:
/// - `1.0` — identical direction
/// - `0.0` — orthogonal (unrelated)
/// - `-1.0` — opposite direction
///
/// Returns `0.0` for zero-length vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (ai, bi) in a.iter().zip(b.iter()) {
        let ai = *ai as f64;
        let bi = *bi as f64;
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }

    let denominator = (norm_a.sqrt()) * (norm_b.sqrt());
    if denominator < f64::EPSILON {
        return 0.0;
    }

    (dot / denominator) as f32
}

/// Compute recency factor using exponential decay.
///
/// Formula: `e^(-decay_rate * hours_since_access)`
///
/// - `decay_rate = 0.0` → always returns `1.0` (no decay)
/// - Higher `decay_rate` → faster decay
/// - Recent access → higher factor
///
/// Returns `1.0` if `last_accessed` is in the future.
pub fn recency_factor(decay_rate: f32, last_accessed: &Timestamp, now: &Timestamp) -> f32 {
    if decay_rate <= 0.0 {
        return 1.0;
    }

    let duration = *now - *last_accessed;
    let hours = duration.num_seconds().max(0) as f64 / 3600.0;

    (-(decay_rate as f64) * hours).exp() as f32
}

/// Compute the composite retrieval score.
///
/// Formula: `similarity * importance * recency_factor`
///
/// All components are in `[0.0, 1.0]` range (similarity is clamped to non-negative).
pub fn composite_score(similarity: f32, importance: f32, recency: f32) -> f32 {
    similarity.max(0.0) * importance * recency
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            (sim - 1.0).abs() < 0.001,
            "Identical vectors should have similarity 1.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_identical_complex() {
        let a = vec![0.5, 0.3, 0.7, 0.1];
        let b = vec![0.5, 0.3, 0.7, 0.1];
        let sim = cosine_similarity(&a, &b);
        assert!(
            (sim - 1.0).abs() < 0.001,
            "Same vectors should be 1.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            sim.abs() < 0.001,
            "Orthogonal vectors should have similarity 0.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            (sim - (-1.0)).abs() < 0.001,
            "Opposite vectors should have similarity -1.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            sim.abs() < 0.001,
            "Zero vector should yield 0.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_mismatched_length() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0, "Mismatched lengths should return 0.0");
    }

    #[test]
    fn test_recency_factor_no_decay() {
        let now = alpha_common::types::now();
        let past = now - Duration::hours(24);
        let factor = recency_factor(0.0, &past, &now);
        assert!(
            (factor - 1.0).abs() < 0.001,
            "Zero decay should always return 1.0, got {factor}"
        );
    }

    #[test]
    fn test_recency_factor_recent() {
        let now = alpha_common::types::now();
        let just_now = now - Duration::minutes(5);
        let factor = recency_factor(0.01, &just_now, &now);
        assert!(
            factor > 0.99,
            "Very recent access should have factor near 1.0, got {factor}"
        );
    }

    #[test]
    fn test_recency_factor_old() {
        let now = alpha_common::types::now();
        let long_ago = now - Duration::hours(1000);
        let factor = recency_factor(0.01, &long_ago, &now);
        assert!(
            factor < 0.001,
            "Very old access should have factor near 0.0, got {factor}"
        );
    }

    #[test]
    fn test_recency_factor_future() {
        let now = alpha_common::types::now();
        let future = now + Duration::hours(10);
        let factor = recency_factor(0.01, &future, &now);
        assert!(
            (factor - 1.0).abs() < 0.001,
            "Future timestamp should clamp to 1.0, got {factor}"
        );
    }

    #[test]
    fn test_composite_score() {
        let score = composite_score(0.8, 0.9, 0.7);
        let expected = 0.8 * 0.9 * 0.7;
        assert!(
            (score - expected).abs() < 0.001,
            "Expected {expected}, got {score}"
        );
    }

    #[test]
    fn test_composite_score_negative_similarity_clamped() {
        let score = composite_score(-0.5, 0.9, 1.0);
        assert!(
            score.abs() < 0.001,
            "Negative similarity should be clamped to 0.0, got {score}"
        );
    }

    #[test]
    fn test_composite_score_perfect() {
        let score = composite_score(1.0, 1.0, 1.0);
        assert!(
            (score - 1.0).abs() < 0.001,
            "All maximal inputs should give 1.0, got {score}"
        );
    }
}
