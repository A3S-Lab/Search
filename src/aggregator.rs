//! Result aggregation and ranking.

use std::collections::HashMap;

use crate::{SearchResult, SearchResults};

/// Result priority for ranking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ResultPriority {
    High,
    #[default]
    Normal,
    Low,
}

/// Aggregates and ranks search results from multiple engines.
#[derive(Debug, Default)]
pub struct Aggregator {
    /// Engine weights for scoring.
    engine_weights: HashMap<String, f64>,
}

impl Aggregator {
    /// Creates a new aggregator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the weight for an engine.
    pub fn set_engine_weight(&mut self, engine: impl Into<String>, weight: f64) {
        self.engine_weights.insert(engine.into(), weight);
    }

    /// Aggregates results from multiple engines.
    ///
    /// This performs:
    /// 1. Deduplication based on normalized URL
    /// 2. Merging of duplicate results (combining engines and positions)
    /// 3. Score calculation
    /// 4. Sorting by score
    pub fn aggregate(&self, engine_results: Vec<(String, Vec<SearchResult>)>) -> SearchResults {
        let mut url_map: HashMap<String, SearchResult> = HashMap::new();

        for (engine_name, results) in engine_results {
            for (position, mut result) in results.into_iter().enumerate() {
                let normalized = result.normalized_url();
                let position = (position + 1) as u32;

                if let Some(existing) = url_map.get_mut(&normalized) {
                    self.merge_results(existing, result, &engine_name, position);
                } else {
                    result.engines.insert(engine_name.clone());
                    result.positions.push(position);
                    url_map.insert(normalized, result);
                }
            }
        }

        let mut results: Vec<SearchResult> = url_map.into_values().collect();

        for result in &mut results {
            result.score = self.calculate_score(result, ResultPriority::Normal);
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut search_results = SearchResults::new();
        for result in results {
            search_results.add_result(result);
        }
        search_results
    }

    /// Merges a new result into an existing one.
    fn merge_results(
        &self,
        existing: &mut SearchResult,
        new: SearchResult,
        engine: &str,
        position: u32,
    ) {
        existing.engines.insert(engine.to_string());
        existing.positions.push(position);

        if new.title.len() > existing.title.len() {
            existing.title = new.title;
        }
        if new.content.len() > existing.content.len() {
            existing.content = new.content;
        }
        if existing.thumbnail.is_none() && new.thumbnail.is_some() {
            existing.thumbnail = new.thumbnail;
        }
        if existing.published_date.is_none() && new.published_date.is_some() {
            existing.published_date = new.published_date;
        }
    }

    /// Calculates the score for a result.
    ///
    /// The scoring algorithm is based on SearXNG:
    /// - Weight is multiplied by engine weights
    /// - Weight is multiplied by number of engines that found the result
    /// - Score is sum of (weight / position) for each position
    fn calculate_score(&self, result: &SearchResult, priority: ResultPriority) -> f64 {
        let mut weight = 1.0;

        for engine in &result.engines {
            weight *= self.engine_weights.get(engine).copied().unwrap_or(1.0);
        }

        weight *= result.engines.len() as f64;

        let mut score = 0.0;
        for &position in &result.positions {
            match priority {
                ResultPriority::High => score += weight,
                ResultPriority::Normal => score += weight / position as f64,
                ResultPriority::Low => {}
            }
        }

        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_priority_default() {
        let default: ResultPriority = Default::default();
        assert_eq!(default, ResultPriority::Normal);
    }

    #[test]
    fn test_aggregator_new() {
        let aggregator = Aggregator::new();
        assert!(aggregator.engine_weights.is_empty());
    }

    #[test]
    fn test_aggregator_set_engine_weight() {
        let mut aggregator = Aggregator::new();
        aggregator.set_engine_weight("google", 1.5);
        aggregator.set_engine_weight("bing", 1.2);
        assert_eq!(aggregator.engine_weights.get("google"), Some(&1.5));
        assert_eq!(aggregator.engine_weights.get("bing"), Some(&1.2));
    }

    #[test]
    fn test_aggregate_empty_results() {
        let aggregator = Aggregator::new();
        let engine_results: Vec<(String, Vec<SearchResult>)> = vec![];
        let aggregated = aggregator.aggregate(engine_results);
        assert_eq!(aggregated.count, 0);
    }

    #[test]
    fn test_aggregate_single_engine() {
        let aggregator = Aggregator::new();
        let results = vec![SearchResult::new("https://example.com", "Title", "Content")];
        let engine_results = vec![("engine1".to_string(), results)];
        let aggregated = aggregator.aggregate(engine_results);
        assert_eq!(aggregated.count, 1);
        assert!(aggregated.items()[0].engines.contains("engine1"));
    }

    #[test]
    fn test_aggregate_deduplicates_by_url() {
        let aggregator = Aggregator::new();

        let results1 = vec![
            SearchResult::new("https://example.com/page", "Title 1", "Content 1"),
            SearchResult::new("https://other.com", "Other", "Other content"),
        ];
        let results2 = vec![SearchResult::new(
            "http://example.com/page/",
            "Title 2 Longer",
            "Content 2",
        )];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);

        assert_eq!(aggregated.items().len(), 2);

        let example_result = aggregated
            .items()
            .iter()
            .find(|r| r.normalized_url() == "example.com/page")
            .unwrap();
        assert_eq!(example_result.engines.len(), 2);
        assert!(example_result.engines.contains("engine1"));
        assert!(example_result.engines.contains("engine2"));
        assert_eq!(example_result.title, "Title 2 Longer");
    }

    #[test]
    fn test_aggregate_merges_longer_content() {
        let aggregator = Aggregator::new();

        let results1 = vec![SearchResult::new(
            "https://example.com",
            "Short",
            "Short content",
        )];
        let results2 = vec![SearchResult::new(
            "https://example.com",
            "Longer Title Here",
            "Much longer content description",
        )];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);
        let result = &aggregated.items()[0];

        assert_eq!(result.title, "Longer Title Here");
        assert_eq!(result.content, "Much longer content description");
    }

    #[test]
    fn test_aggregate_merges_thumbnail() {
        let aggregator = Aggregator::new();

        let results1 = vec![SearchResult::new("https://example.com", "Title", "Content")];
        let results2 = vec![SearchResult::new("https://example.com", "Title", "Content")
            .with_thumbnail("https://example.com/thumb.jpg")];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);
        let result = &aggregated.items()[0];

        assert_eq!(
            result.thumbnail,
            Some("https://example.com/thumb.jpg".to_string())
        );
    }

    #[test]
    fn test_aggregate_merges_published_date() {
        let aggregator = Aggregator::new();

        let results1 = vec![SearchResult::new("https://example.com", "Title", "Content")];
        let results2 = vec![SearchResult::new("https://example.com", "Title", "Content")
            .with_published_date("2024-01-15")];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);
        let result = &aggregated.items()[0];

        assert_eq!(result.published_date, Some("2024-01-15".to_string()));
    }

    #[test]
    fn test_score_calculation() {
        let mut aggregator = Aggregator::new();
        aggregator.set_engine_weight("engine1", 2.0);

        let results1 = vec![SearchResult::new("https://example.com", "Title", "Content")];
        let results2 = vec![SearchResult::new("https://example.com", "Title", "Content")];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);
        let result = &aggregated.items()[0];

        assert!(result.score > 0.0);
        assert_eq!(result.engines.len(), 2);
    }

    #[test]
    fn test_results_sorted_by_score() {
        let mut aggregator = Aggregator::new();
        aggregator.set_engine_weight("engine1", 1.0);
        aggregator.set_engine_weight("engine2", 1.0);

        // Result found by both engines should rank higher
        let results1 = vec![
            SearchResult::new("https://single.com", "Single", "Found by one"),
            SearchResult::new("https://both.com", "Both", "Found by both"),
        ];
        let results2 = vec![SearchResult::new(
            "https://both.com",
            "Both",
            "Found by both",
        )];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);

        // The result found by both engines should be first
        assert_eq!(aggregated.items()[0].engines.len(), 2);
    }

    #[test]
    fn test_position_affects_score() {
        let aggregator = Aggregator::new();

        // First position should score higher than later positions
        let results = vec![
            SearchResult::new("https://first.com", "First", "Position 1"),
            SearchResult::new("https://second.com", "Second", "Position 2"),
            SearchResult::new("https://third.com", "Third", "Position 3"),
        ];

        let engine_results = vec![("engine1".to_string(), results)];
        let aggregated = aggregator.aggregate(engine_results);

        // Results should maintain order based on position score
        assert!(aggregated.items()[0].score >= aggregated.items()[1].score);
        assert!(aggregated.items()[1].score >= aggregated.items()[2].score);
    }

    #[test]
    fn test_engine_weight_affects_score() {
        let mut aggregator = Aggregator::new();
        aggregator.set_engine_weight("high_weight", 3.0);
        aggregator.set_engine_weight("low_weight", 0.5);

        let results_high = vec![SearchResult::new(
            "https://high.com",
            "High",
            "From high weight engine",
        )];
        let results_low = vec![SearchResult::new(
            "https://low.com",
            "Low",
            "From low weight engine",
        )];

        let engine_results = vec![
            ("high_weight".to_string(), results_high),
            ("low_weight".to_string(), results_low),
        ];

        let aggregated = aggregator.aggregate(engine_results);

        let high_result = aggregated
            .items()
            .iter()
            .find(|r| r.url == "https://high.com")
            .unwrap();
        let low_result = aggregated
            .items()
            .iter()
            .find(|r| r.url == "https://low.com")
            .unwrap();

        assert!(high_result.score > low_result.score);
    }

    #[test]
    fn test_aggregate_preserves_positions() {
        let aggregator = Aggregator::new();

        let results1 = vec![SearchResult::new("https://example.com", "Title", "Content")];
        let results2 = vec![
            SearchResult::new("https://other.com", "Other", "Other"),
            SearchResult::new("https://example.com", "Title", "Content"),
        ];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);
        let example_result = aggregated
            .items()
            .iter()
            .find(|r| r.normalized_url() == "example.com")
            .unwrap();

        // Position 1 from engine1, position 2 from engine2
        assert_eq!(example_result.positions.len(), 2);
        assert!(example_result.positions.contains(&1));
        assert!(example_result.positions.contains(&2));
    }

    #[test]
    fn test_result_priority_variants() {
        assert_eq!(ResultPriority::Normal, ResultPriority::default());
        assert_ne!(ResultPriority::High, ResultPriority::Normal);
        assert_ne!(ResultPriority::Low, ResultPriority::Normal);
        assert_ne!(ResultPriority::High, ResultPriority::Low);
    }

    #[test]
    fn test_calculate_score_high_priority() {
        let aggregator = Aggregator::new();
        let mut result = SearchResult::new("https://example.com", "Title", "Content");
        result.engines.insert("engine1".to_string());
        result.positions.push(5);

        let score = aggregator.calculate_score(&result, ResultPriority::High);
        // High priority: score = weight (not divided by position)
        assert!(score > 0.0);
        // For High priority, position doesn't reduce score
        let score_pos1 = {
            let mut r = SearchResult::new("https://example.com", "Title", "Content");
            r.engines.insert("engine1".to_string());
            r.positions.push(1);
            aggregator.calculate_score(&r, ResultPriority::High)
        };
        assert_eq!(score, score_pos1, "High priority should ignore position");
    }

    #[test]
    fn test_calculate_score_low_priority() {
        let aggregator = Aggregator::new();
        let mut result = SearchResult::new("https://example.com", "Title", "Content");
        result.engines.insert("engine1".to_string());
        result.positions.push(1);

        let score = aggregator.calculate_score(&result, ResultPriority::Low);
        assert_eq!(score, 0.0, "Low priority should always score 0");
    }

    #[test]
    fn test_calculate_score_no_engine_weight() {
        let aggregator = Aggregator::new();
        let mut result = SearchResult::new("https://example.com", "Title", "Content");
        result.engines.insert("unknown_engine".to_string());
        result.positions.push(1);

        let score = aggregator.calculate_score(&result, ResultPriority::Normal);
        // Default weight is 1.0, 1 engine, position 1: score = 1.0 * 1 / 1 = 1.0
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_aggregator_debug() {
        let aggregator = Aggregator::new();
        let debug_str = format!("{:?}", aggregator);
        assert!(debug_str.contains("Aggregator"));
    }

    #[test]
    fn test_aggregate_merges_longer_title() {
        let aggregator = Aggregator::new();

        let results1 = vec![SearchResult::new("https://example.com", "Short", "Content")];
        let results2 = vec![SearchResult::new(
            "https://example.com",
            "Much Longer Title",
            "Content",
        )];

        let engine_results = vec![
            ("engine1".to_string(), results1),
            ("engine2".to_string(), results2),
        ];

        let aggregated = aggregator.aggregate(engine_results);
        assert_eq!(aggregated.items()[0].title, "Much Longer Title");
    }
}
