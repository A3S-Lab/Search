use super::*;
use crate::Aggregator;

fn result(url: &str, title: &str, content: &str) -> SearchResult {
    SearchResult::new(url, title, content)
}

#[test]
fn query_alignment_is_domain_and_language_neutral() {
    let english = result(
        "https://docs.example/async-traits",
        "Async functions in traits",
        "Official language reference for async trait methods",
    );
    let english_noise = result(
        "https://example.test/game",
        "A survival game",
        "Unrelated entertainment page",
    );
    assert!(
        query_match_score("async fn in traits official reference", &english)
            > query_match_score("async fn in traits official reference", &english_noise)
    );

    let chinese = result(
        "https://example.cn/transport",
        "跨境交通运行评估",
        "这份报告披露赛事期间跨境交通的运行表现",
    );
    let chinese_noise = result(
        "https://example.cn/food",
        "城市餐饮指南",
        "介绍本地餐厅和菜单",
    );
    assert!(
        query_match_score("跨境交通运行表现报告", &chinese)
            > query_match_score("跨境交通运行表现报告", &chinese_noise)
    );
}

#[test]
fn query_aware_aggregation_demotes_low_alignment_capacity() {
    let aggregator = Aggregator::new();
    let ranked = aggregator.aggregate_for_query(
        "malaria vaccine position paper",
        vec![(
            "engine".to_string(),
            vec![
                result(
                    "https://noise.example/world",
                    "World news",
                    "General headlines",
                ),
                result(
                    "https://evidence.example/malaria-vaccine",
                    "Malaria vaccine position paper",
                    "Technical recommendation and evidence review",
                ),
            ],
        )],
    );

    assert_eq!(
        ranked.items()[0].url,
        "https://evidence.example/malaria-vaccine"
    );
    assert!(
        ranked.items()[0].query_match_score.unwrap() > ranked.items()[1].query_match_score.unwrap()
    );
}

#[test]
fn cascade_runs_lower_tier_only_until_quality_floor_is_met() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = SearchQuery::new("async trait reference");
    let aggregator = Aggregator::new();
    let mut cascade = SearchCascade::new(query, floor);

    let api = aggregator.aggregate_for_query(
        "async trait reference",
        vec![(
            "api".to_string(),
            vec![result(
                "https://noise.example/",
                "General programming news",
                "A broad index",
            )],
        )],
    );
    assert_eq!(cascade.push_tier("api", api), SearchTierDecision::Continue);

    let http = aggregator.aggregate_for_query(
        "async trait reference",
        vec![(
            "http".to_string(),
            vec![
                result(
                    "https://reference.example/async-trait",
                    "Async trait reference",
                    "Language reference",
                ),
                result(
                    "https://guide.example/async-trait",
                    "Async trait guide",
                    "Reference guide",
                ),
            ],
        )],
    );
    assert_eq!(cascade.push_tier("http", http), SearchTierDecision::Stop);
    assert!(!cascade.needs_next_tier());
    assert_eq!(cascade.reports().len(), 2);
}

#[test]
fn strict_generic_floor_can_require_consensus_and_mean_alignment() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 2,
        min_aligned_results: 2,
        min_consensus_results: 1,
        min_query_match: 0.2,
        min_mean_query_match: 0.5,
    };
    let insufficient = SearchQuality {
        usable_result_count: 2,
        unique_host_count: 2,
        contributing_engine_count: 2,
        consensus_result_count: 0,
        aligned_result_count: 2,
        mean_query_match: 0.75,
    };
    assert!(!floor.is_met(&insufficient));

    let sufficient = SearchQuality {
        consensus_result_count: 1,
        ..insufficient
    };
    assert!(floor.is_met(&sufficient));
}

#[test]
fn non_finite_programmatic_alignment_is_recomputed() {
    let mut item = result(
        "https://example.test/async-trait",
        "Async trait reference",
        "Language reference",
    );
    item.query_match_score = Some(f64::NAN);
    let mut results = SearchResults::new();
    results.add_result(item);

    let quality = SearchQuality::evaluate("async trait reference", &results, 0.2);

    assert!(quality.mean_query_match.is_finite());
    assert_eq!(quality.aligned_result_count, 1);
}

#[test]
fn tier_merge_deduplicates_urls_and_preserves_independent_provenance() {
    let aggregator = Aggregator::new();
    let first = aggregator.aggregate_for_query(
        "shared evidence",
        vec![(
            "api".to_string(),
            vec![result(
                "https://example.com/report?utm_source=api",
                "Shared evidence",
                "Short",
            )],
        )],
    );
    let second = aggregator.aggregate_for_query(
        "shared evidence",
        vec![(
            "headless".to_string(),
            vec![result(
                "https://www.example.com/report",
                "Shared evidence report",
                "A richer description of the shared evidence",
            )],
        )],
    );

    let mut cascade = SearchCascade::new(SearchQuery::new("shared evidence"), Default::default());
    cascade.push_tier("api", first);
    cascade.push_tier("headless", second);

    assert_eq!(cascade.results().items().len(), 1);
    let merged = &cascade.results().items()[0];
    assert_eq!(merged.engines.len(), 2);
    assert!(merged.engines.contains("api"));
    assert!(merged.engines.contains("headless"));
    assert!(merged.content.contains("richer"));
}
