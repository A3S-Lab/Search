//! Result aggregation and ranking.

use std::collections::{BTreeMap, HashMap};

use crate::{SearchResult, SearchResults};

#[derive(Debug, Clone, Copy)]
struct RankSignal {
    position: u32,
    relevance: Option<f64>,
    contribution: f64,
}

#[derive(Debug)]
struct Candidate {
    result: SearchResult,
    signals: BTreeMap<String, RankSignal>,
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
        let weight = if weight.is_finite() && weight >= 0.0 {
            weight
        } else {
            1.0
        };
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
        let mut url_map: HashMap<String, Candidate> = HashMap::new();

        for (engine_name, results) in engine_results {
            for (position, mut result) in results.into_iter().enumerate() {
                let normalized = result.normalized_url();
                let position = (position + 1) as u32;
                let relevance = normalized_relevance(result.relevance_score);
                let contribution = self.engine_weight(&engine_name) * relevance.unwrap_or(1.0)
                    / f64::from(position);
                let signal = RankSignal {
                    position,
                    relevance,
                    contribution,
                };

                result.engines.clear();
                result.positions.clear();
                result.score = 0.0;

                if let Some(candidate) = url_map.get_mut(&normalized) {
                    merge_results(&mut candidate.result, result);
                    candidate
                        .signals
                        .entry(engine_name.clone())
                        .and_modify(|existing| {
                            if signal.contribution > existing.contribution
                                || (signal.contribution == existing.contribution
                                    && signal.position < existing.position)
                            {
                                *existing = signal;
                            }
                        })
                        .or_insert(signal);
                } else {
                    let mut signals = BTreeMap::new();
                    signals.insert(engine_name.clone(), signal);
                    url_map.insert(normalized, Candidate { result, signals });
                }
            }
        }

        let mut results: Vec<SearchResult> = url_map
            .into_values()
            .map(|mut candidate| {
                candidate.result.engines = candidate.signals.keys().cloned().collect();
                candidate.result.positions = candidate
                    .signals
                    .values()
                    .map(|signal| signal.position)
                    .collect();
                candidate.result.relevance_score = candidate
                    .signals
                    .values()
                    .filter_map(|signal| signal.relevance)
                    .max_by(f64::total_cmp);
                candidate.result.score = candidate
                    .signals
                    .values()
                    .map(|signal| signal.contribution)
                    .fold(0.0, saturating_score_add);
                candidate.result
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| b.engines.len().cmp(&a.engines.len()))
                .then_with(|| a.normalized_url().cmp(&b.normalized_url()))
                .then_with(|| a.title.cmp(&b.title))
        });

        let mut search_results = SearchResults::new();
        for result in results {
            search_results.add_result(result);
        }
        search_results
    }

    fn engine_weight(&self, engine: &str) -> f64 {
        self.engine_weights.get(engine).copied().unwrap_or(1.0)
    }
}

fn normalized_relevance(relevance: Option<f64>) -> Option<f64> {
    match relevance {
        Some(relevance) if relevance.is_finite() => Some(relevance.clamp(0.0, 1.0)),
        _ => None,
    }
}

fn saturating_score_add(total: f64, contribution: f64) -> f64 {
    let score = total + contribution;
    if score.is_finite() {
        score
    } else {
        f64::MAX
    }
}

fn merge_results(existing: &mut SearchResult, new: SearchResult) {
    if better_url(&new.url, &existing.url) {
        existing.url = new.url.clone();
    }
    merge_richer_string(&mut existing.title, new.title);
    merge_richer_string(&mut existing.content, new.content);
    merge_optional_richer_string(&mut existing.full_text, new.full_text);
    merge_optional_stable_string(&mut existing.thumbnail, new.thumbnail);
    merge_optional_latest_string(&mut existing.published_date, new.published_date);
    merge_optional_stable_string(&mut existing.favicon, new.favicon);
    for image in new.images {
        crate::result::merge_image(&mut existing.images, image);
    }

    if matches!(existing.result_type, crate::ResultType::Web)
        && !matches!(new.result_type, crate::ResultType::Web)
    {
        existing.result_type = new.result_type;
    }

    existing.relevance_score = match (
        normalized_relevance(existing.relevance_score),
        normalized_relevance(new.relevance_score),
    ) {
        (Some(existing), Some(new)) => Some(existing.max(new)),
        (Some(existing), None) => Some(existing),
        (None, Some(new)) => Some(new),
        (None, None) => None,
    };
}

fn merge_richer_string(existing: &mut String, new: String) {
    if new.len() > existing.len() || (new.len() == existing.len() && new < *existing) {
        *existing = new;
    }
}

fn merge_optional_richer_string(existing: &mut Option<String>, new: Option<String>) {
    match (existing.as_ref(), new) {
        (None, Some(new)) => *existing = Some(new),
        (Some(current), Some(new))
            if new.len() > current.len() || (new.len() == current.len() && new < *current) =>
        {
            *existing = Some(new);
        }
        _ => {}
    }
}

fn merge_optional_stable_string(existing: &mut Option<String>, new: Option<String>) {
    match (existing.as_ref(), new) {
        (None, Some(new)) => *existing = Some(new),
        (Some(current), Some(new)) if new < *current => *existing = Some(new),
        _ => {}
    }
}

fn merge_optional_latest_string(existing: &mut Option<String>, new: Option<String>) {
    match (existing.as_ref(), new) {
        (None, Some(new)) => *existing = Some(new),
        (Some(current), Some(new)) if new > *current => *existing = Some(new),
        _ => {}
    }
}

fn better_url(candidate: &str, current: &str) -> bool {
    match (
        candidate.starts_with("https://"),
        current.starts_with("https://"),
    ) {
        (true, false) => true,
        (false, true) => false,
        _ => candidate < current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_default_engine_weight_scores_first_position_as_one() {
        let aggregator = Aggregator::new();
        let aggregated = aggregator.aggregate(vec![(
            "unknown_engine".to_string(),
            vec![SearchResult::new("https://example.com", "Title", "Content")],
        )]);

        assert_eq!(aggregated.items()[0].score, 1.0);
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

    #[test]
    fn test_score_is_sum_of_independent_source_contributions() {
        let mut aggregator = Aggregator::new();
        aggregator.set_engine_weight("first", 2.0);
        aggregator.set_engine_weight("second", 0.5);

        let first =
            SearchResult::new("https://example.com", "First", "First").with_relevance_score(0.5);
        let second = vec![
            SearchResult::new("https://other.example", "Other", "Other"),
            SearchResult::new("https://example.com", "Second", "Second").with_relevance_score(0.8),
        ];

        let aggregated = aggregator.aggregate(vec![
            ("first".to_string(), vec![first]),
            ("second".to_string(), second),
        ]);
        let result = aggregated
            .items()
            .iter()
            .find(|result| result.normalized_url() == "example.com")
            .unwrap();

        // first: 2.0 * 0.5 / 1 = 1.0
        // second: 0.5 * 0.8 / 2 = 0.2
        assert!((result.score - 1.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_weights_are_not_multiplied_across_engines() {
        let mut aggregator = Aggregator::new();
        aggregator.set_engine_weight("first", 2.0);
        aggregator.set_engine_weight("second", 3.0);

        let aggregated = aggregator.aggregate(vec![
            (
                "first".to_string(),
                vec![SearchResult::new("https://example.com", "One", "One")],
            ),
            (
                "second".to_string(),
                vec![SearchResult::new("https://example.com", "Two", "Two")],
            ),
        ]);

        assert_eq!(aggregated.items()[0].score, 5.0);
    }

    #[test]
    fn test_extreme_finite_weights_do_not_create_infinite_scores() {
        let mut aggregator = Aggregator::new();
        aggregator.set_engine_weight("first", f64::MAX);
        aggregator.set_engine_weight("second", f64::MAX);

        let aggregated = aggregator.aggregate(vec![
            (
                "first".to_string(),
                vec![SearchResult::new("https://example.com", "One", "One")],
            ),
            (
                "second".to_string(),
                vec![SearchResult::new("https://example.com", "Two", "Two")],
            ),
        ]);

        assert_eq!(aggregated.items()[0].score, f64::MAX);
        assert!(serde_json::to_string(&aggregated).is_ok());
    }

    #[test]
    fn test_same_engine_duplicates_do_not_double_count() {
        let aggregator = Aggregator::new();
        let aggregated = aggregator.aggregate(vec![(
            "engine".to_string(),
            vec![
                SearchResult::new("https://example.com", "One", "One"),
                SearchResult::new("https://example.com/", "Two", "Two"),
            ],
        )]);

        let result = &aggregated.items()[0];
        assert_eq!(result.engines.len(), 1);
        assert_eq!(result.positions, vec![1]);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn test_relevance_is_clamped() {
        let aggregator = Aggregator::new();
        let aggregated = aggregator.aggregate(vec![(
            "engine".to_string(),
            vec![
                SearchResult::new("https://high.example", "High", "High").with_relevance_score(4.0),
                SearchResult::new("https://low.example", "Low", "Low").with_relevance_score(-2.0),
            ],
        )]);

        let high = aggregated
            .items()
            .iter()
            .find(|result| result.url.contains("high"))
            .unwrap();
        let low = aggregated
            .items()
            .iter()
            .find(|result| result.url.contains("low"))
            .unwrap();
        assert_eq!(high.score, 1.0);
        assert_eq!(low.score, 0.0);
    }

    #[test]
    fn test_missing_relevance_remains_unreported() {
        let aggregator = Aggregator::new();
        let aggregated = aggregator.aggregate(vec![(
            "conventional".to_string(),
            vec![SearchResult::new(
                "https://example.com",
                "Example",
                "Snippet",
            )],
        )]);

        assert_eq!(aggregated.items()[0].score, 1.0);
        assert_eq!(aggregated.items()[0].relevance_score, None);
    }

    #[test]
    fn test_missing_relevance_does_not_overwrite_provider_relevance() {
        let aggregator = Aggregator::new();
        let provider = SearchResult::new("https://example.com", "Provider", "Provider")
            .with_relevance_score(0.8);
        let conventional =
            SearchResult::new("https://example.com/", "Conventional", "Conventional");

        let aggregated = aggregator.aggregate(vec![
            ("provider".to_string(), vec![provider]),
            ("conventional".to_string(), vec![conventional]),
        ]);

        assert_eq!(aggregated.items()[0].relevance_score, Some(0.8));
    }

    #[test]
    fn test_merging_preserves_richer_full_text() {
        let aggregator = Aggregator::new();
        let mut short = SearchResult::new("https://example.com", "Title", "Snippet");
        short.full_text = Some("short".to_string());
        let mut rich = SearchResult::new("http://example.com/", "Title", "Snippet");
        rich.full_text = Some("a substantially richer full-text body".to_string());

        let aggregated = aggregator.aggregate(vec![
            ("first".to_string(), vec![short]),
            ("second".to_string(), vec![rich]),
        ]);

        assert_eq!(aggregated.items()[0].url, "https://example.com");
        assert_eq!(
            aggregated.items()[0].full_text.as_deref(),
            Some("a substantially richer full-text body")
        );
    }

    #[test]
    fn test_merging_preserves_favicon_and_deduplicates_images() {
        let aggregator = Aggregator::new();
        let mut first = SearchResult::new("https://example.com", "Title", "Snippet");
        first.favicon = Some("https://example.com/z-icon.ico".to_string());
        first.images = vec![
            crate::SearchImage::new("https://example.com/image.png").with_description("short")
        ];
        let mut second = SearchResult::new("http://example.com/", "Title", "Snippet");
        second.favicon = Some("https://example.com/a-icon.ico".to_string());
        second.images = vec![
            crate::SearchImage::new("https://example.com/image.png")
                .with_description("a richer image description"),
            crate::SearchImage::new("https://example.com/second.png"),
        ];

        let aggregated = aggregator.aggregate(vec![
            ("first".to_string(), vec![first]),
            ("second".to_string(), vec![second]),
        ]);
        let result = &aggregated.items()[0];

        assert_eq!(
            result.favicon.as_deref(),
            Some("https://example.com/a-icon.ico")
        );
        assert_eq!(result.images.len(), 2);
        assert_eq!(
            result.images[0].description.as_deref(),
            Some("a richer image description")
        );
    }

    #[test]
    fn test_ties_are_deterministic() {
        let aggregator = Aggregator::new();
        let forward = aggregator.aggregate(vec![(
            "engine".to_string(),
            vec![
                SearchResult::new("https://b.example", "B", "B"),
                SearchResult::new("https://a.example", "A", "A"),
            ],
        )]);
        let reverse = aggregator.aggregate(vec![(
            "engine".to_string(),
            vec![
                SearchResult::new("https://a.example", "A", "A"),
                SearchResult::new("https://b.example", "B", "B"),
            ],
        )]);

        // Positions are intentionally different here, so compare a true tie.
        let tied_forward = aggregator.aggregate(vec![
            (
                "first".to_string(),
                vec![SearchResult::new("https://b.example", "B", "B")],
            ),
            (
                "second".to_string(),
                vec![SearchResult::new("https://a.example", "A", "A")],
            ),
        ]);
        let tied_reverse = aggregator.aggregate(vec![
            (
                "second".to_string(),
                vec![SearchResult::new("https://a.example", "A", "A")],
            ),
            (
                "first".to_string(),
                vec![SearchResult::new("https://b.example", "B", "B")],
            ),
        ]);

        assert_ne!(forward.items()[0].url, reverse.items()[0].url);
        assert_eq!(tied_forward.items()[0].url, "https://a.example");
        assert_eq!(tied_forward.items()[0].url, tied_reverse.items()[0].url);
    }
}
