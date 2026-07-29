//! Versioned, offline ranking-quality evidence for the public aggregation API.

#[path = "quality_eval/metrics.rs"]
mod metrics;

use std::collections::HashSet;

use a3s_search::{Aggregator, SearchResult, SearchResults};
use metrics::{evaluate_case, AggregateMetrics, CaseMetrics};
use serde::Deserialize;
use serde_json::json;

const CORPUS: &str = include_str!("fixtures/search_quality_v1.json");

#[derive(Debug, Deserialize)]
struct QualityCorpus {
    version: u32,
    cases: Vec<QualityCase>,
}

#[derive(Debug, Deserialize)]
struct QualityCase {
    id: String,
    query: String,
    engines: Vec<FixtureEngine>,
    judgments: Vec<FixtureJudgment>,
}

#[derive(Debug, Deserialize)]
struct FixtureEngine {
    name: String,
    #[serde(default = "default_weight")]
    weight: f64,
    results: Vec<FixtureResult>,
}

#[derive(Debug, Deserialize)]
struct FixtureResult {
    url: String,
    title: String,
    content: String,
    relevance_score: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct FixtureJudgment {
    url: String,
    grade: u8,
}

fn default_weight() -> f64 {
    1.0
}

fn corpus() -> QualityCorpus {
    serde_json::from_str(CORPUS).expect("quality corpus must remain valid JSON")
}

fn rank(case: &QualityCase) -> SearchResults {
    let mut aggregator = Aggregator::new();
    let mut engine_results = Vec::with_capacity(case.engines.len());
    for engine in &case.engines {
        aggregator.set_engine_weight(&engine.name, engine.weight);
        let results = engine
            .results
            .iter()
            .map(|fixture| {
                let result = SearchResult::new(&fixture.url, &fixture.title, &fixture.content);
                match fixture.relevance_score {
                    Some(score) => result.with_relevance_score(score),
                    None => result,
                }
            })
            .collect();
        engine_results.push((engine.name.clone(), results));
    }
    aggregator.aggregate_for_query(&case.query, engine_results)
}

fn evaluate_corpus(corpus: &QualityCorpus) -> Vec<CaseMetrics> {
    corpus
        .cases
        .iter()
        .map(|case| {
            let ranked = rank(case);
            evaluate_case(
                &case.id,
                &ranked,
                case.judgments
                    .iter()
                    .map(|judgment| (normalized_url(&judgment.url), judgment.grade)),
                10,
            )
        })
        .collect()
}

fn normalized_url(url: &str) -> String {
    SearchResult::new(url, "fixture", "fixture").normalized_url()
}

#[test]
fn versioned_quality_corpus_is_well_formed() {
    let corpus = corpus();
    assert_eq!(corpus.version, 1);
    assert!(corpus.cases.len() >= 6, "quality corpus is too narrow");
    assert!(
        corpus.cases.iter().any(|case| !case.query.is_ascii()),
        "quality corpus must retain at least one non-ASCII query"
    );

    let mut case_ids = HashSet::new();
    for case in &corpus.cases {
        assert!(case_ids.insert(case.id.as_str()), "duplicate case ID");
        assert!(!case.query.trim().is_empty());
        assert!(
            case.engines.len() >= 2,
            "{} lacks source diversity",
            case.id
        );
        assert!(!case.judgments.is_empty());

        let mut engine_names = HashSet::new();
        for engine in &case.engines {
            assert!(engine_names.insert(engine.name.as_str()));
            assert!(engine.weight.is_finite() && engine.weight > 0.0);
            assert!(!engine.results.is_empty());
            for result in &engine.results {
                assert!(url::Url::parse(&result.url).is_ok());
                assert!(!result.title.trim().is_empty());
                if let Some(score) = result.relevance_score {
                    assert!(score.is_finite());
                }
            }
        }

        let mut judged_urls = HashSet::new();
        for judgment in &case.judgments {
            assert!(
                judgment.grade <= 3,
                "judgment grades use the closed 0..=3 scale"
            );
            assert!(judged_urls.insert(normalized_url(&judgment.url)));
        }
        assert!(case.judgments.iter().any(|judgment| judgment.grade > 0));
    }
}

#[test]
fn default_ranker_produces_finite_quality_metrics() {
    let corpus = corpus();
    for metrics in evaluate_corpus(&corpus) {
        metrics.assert_finite();
    }
}

#[test]
fn default_ranker_meets_the_versioned_v1_regression_floor() {
    let corpus = corpus();
    let cases = evaluate_corpus(&corpus);
    let aggregate = AggregateMetrics::from_cases(&cases);

    assert!(aggregate.mean_ndcg_at_k >= 0.95, "{aggregate:?}");
    assert!(
        aggregate.mean_reciprocal_rank_at_grade_2 >= 0.99,
        "{aggregate:?}"
    );
    assert_eq!(aggregate.mean_duplicate_ratio_at_k, 0.0);
    assert!(cases.iter().all(|case| case.ndcg_at_k >= 0.74), "{cases:?}");
}

#[test]
#[ignore = "manual versioned ranking-quality evidence; run explicitly"]
fn ranking_quality_report() {
    let corpus = corpus();
    let cases = evaluate_corpus(&corpus);
    let aggregate = AggregateMetrics::from_cases(&cases);

    println!(
        "SEARCH_QUALITY_REPORT={}",
        json!({
            "corpus_version": corpus.version,
            "ranker": "default",
            "aggregate": aggregate,
            "cases": cases,
        })
    );
}
