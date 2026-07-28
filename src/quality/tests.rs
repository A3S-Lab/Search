use super::*;
use crate::Aggregator;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
fn multi_term_alignment_rejects_generic_word_and_boilerplate_matches() {
    let query = "global renewable energy outlook IEA IRENA World Bank policy reports";
    let generic = result(
        "https://dictionary.example/global",
        "GLOBAL Definition & Meaning",
        "A word used for the whole world. Send a report if this entry has a problem.",
    );
    let specific = result(
        "https://evidence.example/world-energy-outlook",
        "World Energy Outlook - IEA",
        "Renewable energy policy report with IRENA and World Bank evidence.",
    );

    assert!(query_match_score(query, &generic) < 0.18);
    assert!(query_match_score(query, &specific) >= 0.18);
}

#[test]
fn default_floor_rejects_partial_capacity_and_weak_set_averages() {
    let query = "distributed tracing baggage propagation sampling specification";
    let floor = SearchQualityFloor::for_limit(5);
    let aggregate =
        |results| Aggregator::new().aggregate_for_query(query, vec![("api".to_string(), results)]);
    let urls = [
        "https://one.example/article",
        "https://two.example/article",
        "https://three.example/article",
        "https://four.example/article",
        "https://five.example/article",
    ];

    let shallow = aggregate(
        urls.iter()
            .map(|url| {
                result(
                    url,
                    "Distributed tracing overview",
                    "An introduction to distributed tracing",
                )
            })
            .collect(),
    );
    let shallow_quality = SearchQuality::evaluate(query, &shallow, floor.min_query_match);
    assert_eq!(shallow_quality.usable_result_count, 5);
    assert_eq!(shallow_quality.unique_host_count, 5);
    assert_eq!(shallow_quality.aligned_result_count, 0);
    assert!(!floor.is_met(&shallow_quality));

    let mixed = aggregate(
        urls.iter()
            .enumerate()
            .map(|(index, url)| {
                if index < 3 {
                    result(
                        url,
                        "Distributed tracing baggage",
                        "Distributed tracing baggage guidance",
                    )
                } else {
                    result(url, "Unrelated article", "General introduction")
                }
            })
            .collect(),
    );
    let mixed_quality = SearchQuality::evaluate(query, &mixed, floor.min_query_match);
    assert_eq!(mixed_quality.aligned_result_count, 3);
    assert!(mixed_quality.mean_query_match < floor.min_mean_query_match);
    assert!(!floor.is_met(&mixed_quality));

    let strong = aggregate(
        urls.iter()
            .map(|url| {
                result(
                    url,
                    "Distributed tracing baggage propagation specification",
                    "Sampling requirements for distributed tracing baggage propagation",
                )
            })
            .collect(),
    );
    let strong_quality = SearchQuality::evaluate(query, &strong, floor.min_query_match);
    assert!(floor.is_met(&strong_quality));
}

#[test]
fn repeated_query_terms_do_not_inflate_alignment() {
    let once = query_match_score(
        "distributed tracing sampling specification",
        &result(
            "https://example.test/tracing",
            "Distributed tracing specification",
            "Sampling semantics",
        ),
    );
    let repeated = query_match_score(
        "distributed distributed tracing sampling specification",
        &result(
            "https://example.test/tracing",
            "Distributed tracing specification",
            "Sampling semantics",
        ),
    );

    assert_eq!(once, repeated);
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

#[tokio::test]
async fn lazy_cascade_does_not_initialize_http_or_headless_after_api_quality() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = "distributed tracing sampling specification";
    let aggregator = Aggregator::new();
    let api_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "api".to_string(),
            vec![
                result(
                    "https://reference.example/tracing",
                    "Distributed tracing specification",
                    "Sampling semantics and propagation rules",
                ),
                result(
                    "https://guide.example/sampling",
                    "Tracing sampling guide",
                    "Distributed trace sampling specification",
                ),
            ],
        )],
    );
    let api_calls = Arc::new(AtomicUsize::new(0));
    let http_calls = Arc::new(AtomicUsize::new(0));
    let headless_calls = Arc::new(AtomicUsize::new(0));
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor);

    let calls = Arc::clone(&api_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("api", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                api_results
            })
            .await,
        Some(SearchTierDecision::Stop)
    );

    let calls = Arc::clone(&http_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("http", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                SearchResults::new()
            })
            .await,
        None
    );
    let calls = Arc::clone(&headless_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("headless", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                SearchResults::new()
            })
            .await,
        None
    );

    assert_eq!(api_calls.load(Ordering::SeqCst), 1);
    assert_eq!(http_calls.load(Ordering::SeqCst), 0);
    assert_eq!(headless_calls.load(Ordering::SeqCst), 0);
    assert_eq!(cascade.reports().len(), 1);
}

#[tokio::test]
async fn lazy_cascade_runs_http_after_api_failure_but_stops_before_headless() {
    let floor = SearchQualityFloor {
        min_usable_results: 2,
        min_unique_hosts: 2,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = "malaria vaccine position paper";
    let mut api_failure = SearchResults::new();
    api_failure.add_failure(
        crate::EngineFailure::new("api", "provider_quota", "quota exhausted").with_transient(false),
    );
    let http_results = Aggregator::new().aggregate_for_query(
        query,
        vec![(
            "http".to_string(),
            vec![
                result(
                    "https://health.example/malaria-vaccine",
                    "Malaria vaccine position paper",
                    "Evidence and recommendation",
                ),
                result(
                    "https://policy.example/vaccine-paper",
                    "Vaccine position paper",
                    "Malaria evidence review",
                ),
            ],
        )],
    );
    let api_calls = Arc::new(AtomicUsize::new(0));
    let http_calls = Arc::new(AtomicUsize::new(0));
    let headless_calls = Arc::new(AtomicUsize::new(0));
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor);

    let calls = Arc::clone(&api_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("api", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                api_failure
            })
            .await,
        Some(SearchTierDecision::Continue)
    );
    let calls = Arc::clone(&http_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("http", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                http_results
            })
            .await,
        Some(SearchTierDecision::Stop)
    );
    let calls = Arc::clone(&headless_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("headless", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                SearchResults::new()
            })
            .await,
        None
    );

    assert_eq!(api_calls.load(Ordering::SeqCst), 1);
    assert_eq!(http_calls.load(Ordering::SeqCst), 1);
    assert_eq!(headless_calls.load(Ordering::SeqCst), 0);
    assert_eq!(cascade.reports().len(), 2);
}

#[tokio::test]
async fn lazy_cascade_reaches_headless_only_after_two_insufficient_tiers() {
    let floor = SearchQualityFloor {
        min_usable_results: 3,
        min_unique_hosts: 3,
        min_contributing_engines: 1,
        min_aligned_results: 2,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    };
    let query = "cross border rail capacity assessment";
    let aggregator = Aggregator::new();
    let api_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "api".to_string(),
            vec![result(
                "https://index.example/rail",
                "Rail index",
                "General transport links",
            )],
        )],
    );
    let http_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "http".to_string(),
            vec![result(
                "https://brief.example/capacity",
                "Rail capacity brief",
                "Cross border capacity summary",
            )],
        )],
    );
    let headless_results = aggregator.aggregate_for_query(
        query,
        vec![(
            "headless".to_string(),
            vec![
                result(
                    "https://assessment.example/rail-capacity",
                    "Cross border rail capacity assessment",
                    "Methods and findings",
                ),
                result(
                    "https://evidence.example/cross-border-rail",
                    "Cross border rail evidence",
                    "Capacity assessment results",
                ),
            ],
        )],
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor);

    for (tier, results) in [
        ("api", api_results),
        ("http", http_results),
        ("headless", headless_results),
    ] {
        let calls = Arc::clone(&calls);
        cascade
            .run_tier_if_needed(tier, || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                results
            })
            .await
            .expect("each insufficient predecessor must activate the next tier");
    }

    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(!cascade.needs_next_tier());
    assert_eq!(cascade.reports().len(), 3);
    assert_eq!(cascade.reports()[2].decision, SearchTierDecision::Stop);
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
