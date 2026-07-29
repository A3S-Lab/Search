//! Versioned, offline ranking-quality evidence for the public aggregation API.

#[path = "quality_eval/metrics.rs"]
mod metrics;

use std::collections::HashSet;
use std::path::Path;

use a3s_search::{Aggregator, SearchResult, SearchResults};
use metrics::{evaluate_case, AggregateMetrics, CaseMetrics};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

const CORPUS: &str = include_str!("fixtures/search_quality_v1.json");
const HOLDOUT_PATH_ENV: &str = "A3S_SEARCH_QUALITY_HOLDOUT";
const HOLDOUT_MIN_CASES: usize = 40;
const HOLDOUT_MIN_MEAN_NDCG_AT_10: f64 = 0.80;
const HOLDOUT_MIN_MEAN_MRR_AT_GRADE_2: f64 = 0.85;
const HOLDOUT_MIN_MEAN_RECALL_AT_10: f64 = 0.90;
const HOLDOUT_MIN_CASE_NDCG_AT_10: f64 = 0.45;
const HOLDOUT_MAX_MEAN_DUPLICATE_RATIO_AT_10: f64 = 0.01;

#[derive(Debug, Clone, Deserialize)]
struct QualityCorpus {
    version: u32,
    cases: Vec<QualityCase>,
}

#[derive(Debug, Clone, Deserialize)]
struct QualityCase {
    id: String,
    query: String,
    engines: Vec<FixtureEngine>,
    judgments: Vec<FixtureJudgment>,
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureEngine {
    name: String,
    #[serde(default = "default_weight")]
    weight: f64,
    results: Vec<FixtureResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureResult {
    url: String,
    title: String,
    content: String,
    relevance_score: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureJudgment {
    url: String,
    grade: u8,
}

fn default_weight() -> f64 {
    1.0
}

fn corpus() -> QualityCorpus {
    parse_corpus(CORPUS.as_bytes())
}

fn parse_corpus(bytes: &[u8]) -> QualityCorpus {
    serde_json::from_slice(bytes).expect("quality corpus must remain valid JSON")
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

fn ranking_signature(results: &SearchResults) -> Vec<(String, u64)> {
    results
        .items()
        .iter()
        .map(|result| (result.normalized_url(), result.score.to_bits()))
        .collect()
}

fn validate_corpus(corpus: &QualityCorpus, minimum_cases: usize) {
    assert_eq!(corpus.version, 1);
    assert!(
        corpus.cases.len() >= minimum_cases,
        "quality corpus has {} cases; at least {minimum_cases} are required",
        corpus.cases.len()
    );
    assert!(
        corpus.cases.iter().any(|case| case.query.is_ascii()),
        "quality corpus must retain at least one ASCII query"
    );
    assert!(
        corpus.cases.iter().any(|case| !case.query.is_ascii()),
        "quality corpus must retain at least one non-ASCII query"
    );

    let mut case_ids = HashSet::new();
    let mut queries = HashSet::new();
    for case in &corpus.cases {
        assert!(case_ids.insert(case.id.as_str()), "duplicate case ID");
        assert!(!case.query.trim().is_empty());
        assert!(
            queries.insert(case.query.trim().to_lowercase()),
            "{} repeats a query from another case",
            case.id
        );
        assert!(
            case.engines.len() >= 2,
            "{} lacks source diversity",
            case.id
        );
        assert!(!case.judgments.is_empty());

        let mut candidate_urls = HashSet::new();
        let mut engine_names = HashSet::new();
        for engine in &case.engines {
            assert!(engine_names.insert(engine.name.as_str()));
            assert!(engine.weight.is_finite() && engine.weight > 0.0);
            assert!(!engine.results.is_empty());
            for result in &engine.results {
                let url = url::Url::parse(&result.url).expect("fixture URL must be absolute");
                assert!(matches!(url.scheme(), "http" | "https"));
                assert!(!result.title.trim().is_empty());
                if let Some(score) = result.relevance_score {
                    assert!(score.is_finite());
                }
                candidate_urls.insert(normalized_url(&result.url));
            }
        }

        let mut judged_urls = HashSet::new();
        for judgment in &case.judgments {
            assert!(
                judgment.grade <= 3,
                "judgment grades use the closed 0..=3 scale"
            );
            let normalized = normalized_url(&judgment.url);
            assert!(
                candidate_urls.contains(&normalized),
                "{} judges a URL that no provider returned",
                case.id
            );
            assert!(judged_urls.insert(normalized));
        }
        assert!(
            candidate_urls.is_subset(&judged_urls),
            "{} leaves provider candidates unjudged",
            case.id
        );
        assert!(
            case.judgments.iter().any(|judgment| judgment.grade >= 2),
            "{} has no material result",
            case.id
        );
    }
}

fn corpus_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn versioned_quality_corpus_is_well_formed() {
    let corpus = corpus();
    validate_corpus(&corpus, 6);
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
fn ranker_preserves_domain_neutral_metamorphic_invariants() {
    let corpus = corpus();
    let mut trial_count = 0usize;

    for case in &corpus.cases {
        let baseline = ranking_signature(&rank(case));

        let mut reversed_engines = case.clone();
        reversed_engines.engines.reverse();
        assert_eq!(
            baseline,
            ranking_signature(&rank(&reversed_engines)),
            "{} changed when provider input order changed",
            case.id
        );
        trial_count += 1;

        let mut renamed_engines = case.clone();
        for (index, engine) in renamed_engines.engines.iter_mut().enumerate() {
            engine.name = format!("anonymous-provider-{index}");
        }
        assert_eq!(
            baseline,
            ranking_signature(&rank(&renamed_engines)),
            "{} changed when opaque provider identifiers changed",
            case.id
        );
        trial_count += 1;

        let mut rescaled_native_scores = case.clone();
        for engine in &mut rescaled_native_scores.engines {
            for result in &mut engine.results {
                result.relevance_score = result
                    .relevance_score
                    .map(|score| 0.15 + score.clamp(0.0, 1.0) * 0.7);
            }
        }
        assert_eq!(
            baseline,
            ranking_signature(&rank(&rescaled_native_scores)),
            "{} changed under a provider-local monotone score transform",
            case.id
        );
        trial_count += 1;

        let mut repeated_provider_rows = case.clone();
        for engine in &mut repeated_provider_rows.engines {
            if let Some(first) = engine.results.first().cloned() {
                engine.results.insert(1, first);
            }
        }
        assert_eq!(
            baseline,
            ranking_signature(&rank(&repeated_provider_rows)),
            "{} changed when providers repeated canonical results",
            case.id
        );
        trial_count += 1;
    }

    assert!(
        trial_count >= 24,
        "metamorphic gate must exercise more than a handful of transformations"
    );
}

#[test]
#[ignore = "independent holdout is external to the repository; run before release"]
fn independent_holdout_meets_the_predeclared_quality_floor() {
    let path = std::env::var_os(HOLDOUT_PATH_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| panic!("set {HOLDOUT_PATH_ENV} to the sealed holdout JSON path"));
    let bytes = std::fs::read(&path).expect("read sealed search-quality holdout");
    let holdout = parse_corpus(&bytes);
    validate_corpus(&holdout, HOLDOUT_MIN_CASES);

    let public = corpus();
    let public_ids = public
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<HashSet<_>>();
    let public_queries = public
        .cases
        .iter()
        .map(|case| case.query.trim().to_lowercase())
        .collect::<HashSet<_>>();
    assert!(
        holdout
            .cases
            .iter()
            .all(|case| !public_ids.contains(case.id.as_str())),
        "holdout case IDs must not overlap the public development corpus"
    );
    assert!(
        holdout
            .cases
            .iter()
            .all(|case| !public_queries.contains(&case.query.trim().to_lowercase())),
        "holdout queries must not overlap the public development corpus"
    );

    let cases = evaluate_corpus(&holdout);
    let aggregate = AggregateMetrics::from_cases(&cases);
    assert!(
        aggregate.mean_ndcg_at_k >= HOLDOUT_MIN_MEAN_NDCG_AT_10,
        "{aggregate:?}"
    );
    assert!(
        aggregate.mean_reciprocal_rank_at_grade_2 >= HOLDOUT_MIN_MEAN_MRR_AT_GRADE_2,
        "{aggregate:?}"
    );
    assert!(
        aggregate.mean_recall_at_k >= HOLDOUT_MIN_MEAN_RECALL_AT_10,
        "{aggregate:?}"
    );
    assert!(
        aggregate.mean_duplicate_ratio_at_k <= HOLDOUT_MAX_MEAN_DUPLICATE_RATIO_AT_10,
        "{aggregate:?}"
    );
    assert!(
        cases
            .iter()
            .all(|case| case.ndcg_at_k >= HOLDOUT_MIN_CASE_NDCG_AT_10),
        "{cases:?}"
    );

    println!(
        "SEARCH_QUALITY_HOLDOUT_REPORT={}",
        json!({
            "corpus_version": holdout.version,
            "corpus_file": Path::new(&path).file_name().and_then(|name| name.to_str()),
            "corpus_sha256": corpus_sha256(&bytes),
            "ranker": "default",
            "thresholds": {
                "minimum_cases": HOLDOUT_MIN_CASES,
                "minimum_mean_ndcg_at_10": HOLDOUT_MIN_MEAN_NDCG_AT_10,
                "minimum_mean_mrr_at_grade_2": HOLDOUT_MIN_MEAN_MRR_AT_GRADE_2,
                "minimum_mean_recall_at_10": HOLDOUT_MIN_MEAN_RECALL_AT_10,
                "minimum_case_ndcg_at_10": HOLDOUT_MIN_CASE_NDCG_AT_10,
                "maximum_mean_duplicate_ratio_at_10":
                    HOLDOUT_MAX_MEAN_DUPLICATE_RATIO_AT_10,
            },
            "aggregate": aggregate,
            "cases": cases,
        })
    );
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
