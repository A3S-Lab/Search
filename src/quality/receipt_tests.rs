use super::*;
use crate::{
    Aggregator, EngineCategory, EngineFailure, EngineOutcome, EngineOutcomeKind, SafeSearch,
    SearchImage, SearchReport, SearchUsage, TimeRange,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn floor(requisite_results: usize) -> SearchQualityFloor {
    SearchQualityFloor {
        min_usable_results: requisite_results,
        min_unique_hosts: requisite_results,
        min_contributing_engines: usize::from(requisite_results > 0),
        min_aligned_results: requisite_results,
        min_consensus_results: 0,
        min_query_match: 0.2,
        min_mean_query_match: 0.0,
    }
}

fn aggregate(query: &str, engine: &str, urls: &[&str]) -> SearchResults {
    Aggregator::new().aggregate_for_query(
        query,
        vec![(
            engine.to_string(),
            urls.iter()
                .map(|url| SearchResult::new(*url, query, format!("Evidence for {query}")))
                .collect(),
        )],
    )
}

pub(super) fn exhausted_outcome() -> SearchCascadeOutcomeV1 {
    let query = "portable cascade receipt";
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor(3));
    cascade.push_tier(
        "tier-0",
        aggregate(query, "engine-0", &["https://one.example/evidence"]),
    );
    cascade.push_tier("tier-1", SearchResults::new());
    cascade
        .finish_with_tier_plan(["tier-0", "tier-1"])
        .expect("valid exhausted cascade")
}

#[tokio::test]
async fn receipt_preserves_lazy_stop_without_constructing_later_tiers() {
    let query = "generic lazy receipt";
    let later_calls = Arc::new(AtomicUsize::new(0));
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor(1));

    assert_eq!(
        cascade
            .run_tier_if_needed("tier-0", || async {
                aggregate(query, "engine-0", &["https://one.example/evidence"])
            })
            .await,
        Some(SearchTierDecision::Stop)
    );
    let calls = Arc::clone(&later_calls);
    assert_eq!(
        cascade
            .run_tier_if_needed("tier-1", || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                SearchResults::new()
            })
            .await,
        None
    );

    let outcome = cascade
        .finish_with_tier_plan(["tier-0", "tier-1"])
        .expect("valid early-stop receipt");
    assert_eq!(later_calls.load(Ordering::SeqCst), 0);
    assert_eq!(outcome.receipt.executed_tiers.len(), 1);
    assert!(outcome.receipt.quality_floor_met);
    assert!(!outcome.receipt.exhausted_below_floor);
    outcome.validate().expect("self-consistent outcome");
}

#[test]
fn receipt_distinguishes_exhaustion_from_an_incomplete_plan() {
    let exhausted = exhausted_outcome();
    assert!(!exhausted.receipt.quality_floor_met);
    assert!(exhausted.receipt.exhausted_below_floor);

    let mut cascade = SearchCascade::new(SearchQuery::new("incomplete plan"), floor(2));
    cascade.push_tier("tier-0", SearchResults::new());
    let incomplete = cascade
        .finish_with_tier_plan(["tier-0", "tier-1"])
        .expect("an interrupted caller remains auditable");
    assert!(!incomplete.receipt.quality_floor_met);
    assert!(!incomplete.receipt.exhausted_below_floor);
    incomplete.validate().expect("valid incomplete outcome");
}

#[test]
fn receipt_counts_canonical_cross_tier_merge_output() {
    let query = "shared portable evidence";
    let mut first = aggregate(
        query,
        "engine-0",
        &["https://example.com/report?utm_source=first"],
    );
    first.add_failure(EngineFailure::new(
        "engine-0",
        "transient",
        "bounded diagnostic",
    ));
    first.add_outcome(EngineOutcome::completed(
        "Engine 0",
        "engine-0",
        EngineOutcomeKind::Success,
        1,
    ));
    let second = aggregate(query, "engine-1", &["https://www.example.com/report"]);
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor(3));
    cascade.push_tier("tier-0", first);
    cascade.push_tier("tier-1", second);

    let outcome = cascade
        .finish_with_tier_plan(["tier-0", "tier-1"])
        .expect("valid merged receipt");
    assert_eq!(outcome.results.items().len(), 1);
    assert_eq!(outcome.receipt.counts.results, 1);
    assert_eq!(outcome.receipt.counts.failures, 1);
    assert_eq!(outcome.receipt.counts.outcomes, 1);
    outcome.validate().expect("counts bind final output");
}

#[test]
fn receipt_round_trips_through_its_versioned_json_schema() {
    let outcome = exhausted_outcome();
    let expected_wire: serde_json::Value = serde_json::from_str(
        r#"{
          "schema": "a3s/search-cascade-receipt/v1",
          "query": {
            "sha256": "2080566300dae7f22d12435c4de961006d3007d861c36180b1fb06c8868bfd61",
            "value": {
              "query": "portable cascade receipt",
              "categories": ["general"],
              "language": null,
              "safesearch": "Off",
              "page": 1,
              "time_range": null,
              "engines": []
            }
          },
          "quality_floor": {
            "min_usable_results": 3,
            "min_unique_hosts": 3,
            "min_contributing_engines": 1,
            "min_aligned_results": 3,
            "min_consensus_results": 0,
            "min_query_match": 0.2,
            "min_mean_query_match": 0.0
          },
          "final_quality": {
            "usable_result_count": 1,
            "unique_host_count": 1,
            "contributing_engine_count": 1,
            "consensus_result_count": 0,
            "aligned_result_count": 1,
            "mean_query_match": 0.95
          },
          "result_set": {
            "sha256": "7c1342ed154b2880e5c56ec94f45a472ccac1e03ef468be2a21e24c0f9147454"
          },
          "configured_tiers": ["tier-0", "tier-1"],
          "executed_tiers": [
            {
              "tier": "tier-0",
              "combined_quality": {
                "usable_result_count": 1,
                "unique_host_count": 1,
                "contributing_engine_count": 1,
                "consensus_result_count": 0,
                "aligned_result_count": 1,
                "mean_query_match": 0.95
              },
              "decision": "continue"
            },
            {
              "tier": "tier-1",
              "combined_quality": {
                "usable_result_count": 1,
                "unique_host_count": 1,
                "contributing_engine_count": 1,
                "consensus_result_count": 0,
                "aligned_result_count": 1,
                "mean_query_match": 0.95
              },
              "decision": "continue"
            }
          ],
          "quality_floor_met": false,
          "exhausted_below_floor": true,
          "counts": {"results": 1, "failures": 0, "outcomes": 0}
        }"#,
    )
    .expect("decode frozen receipt fixture");
    assert_eq!(
        serde_json::to_value(&outcome.receipt).expect("serialize frozen receipt"),
        expected_wire
    );
    let json = serde_json::to_vec(&outcome).expect("serialize receipt outcome");
    let decoded: SearchCascadeOutcomeV1 =
        serde_json::from_slice(&json).expect("deserialize receipt outcome");

    assert_eq!(decoded.receipt.schema, SEARCH_CASCADE_RECEIPT_V1_SCHEMA);
    decoded.validate().expect("round-tripped outcome validates");

    let mut value = serde_json::to_value(&outcome).expect("serialize receipt value");
    value["receipt"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SearchCascadeOutcomeV1>(value).is_err());

    let mut nested = serde_json::to_value(&outcome).expect("serialize nested receipt value");
    nested["receipt"]["query"]["value"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SearchCascadeOutcomeV1>(nested).is_err());
}

#[test]
fn complete_typed_query_controls_are_bound_by_sha256() {
    let baseline = SearchQuery::new("same query");
    let variants = [
        SearchQuery::new("different query"),
        baseline.clone().with_categories(vec![EngineCategory::News]),
        baseline.clone().with_language("zh-CN"),
        baseline.clone().with_safesearch(SafeSearch::Strict),
        baseline.clone().with_page(2),
        baseline.clone().with_time_range(TimeRange::Week),
        baseline.clone().with_engines(vec!["engine-0".to_string()]),
    ];
    let baseline_binding = SearchQueryBindingV1::new(baseline);
    let digests = variants
        .into_iter()
        .map(|query| SearchQueryBindingV1::new(query).sha256)
        .collect::<HashSet<_>>();

    assert_eq!(digests.len(), 7);
    assert!(!digests.contains(&baseline_binding.sha256));
    baseline_binding.validate().expect("baseline query binding");
}

#[test]
fn query_binding_has_a_frozen_domain_separated_digest() {
    let query = SearchQuery::new("跨语言 evidence")
        .with_categories(vec![EngineCategory::Science, EngineCategory::News])
        .with_language("zh-CN")
        .with_safesearch(SafeSearch::Moderate)
        .with_page(3)
        .with_time_range(TimeRange::Month)
        .with_engines(vec!["engine-a".to_string(), "engine-b".to_string()]);
    let binding = SearchQueryBindingV1::new(query);

    assert_eq!(
        binding.sha256,
        "30fb604123015755746ea9c724c120d41548196394fabe7a90958b63a95af0b9"
    );
}

#[test]
fn receipt_rejects_tampered_query_counts_quality_and_state() {
    let mut schema = exhausted_outcome();
    schema.receipt.schema = "a3s/search-cascade-receipt/v2".to_string();
    assert!(schema.validate().is_err());

    let mut digest = exhausted_outcome();
    digest.receipt.query.sha256 = "0".repeat(64);
    assert!(digest.validate().is_err());

    let mut uppercase_digest = exhausted_outcome();
    uppercase_digest.receipt.query.sha256 = uppercase_digest.receipt.query.sha256.to_uppercase();
    assert!(uppercase_digest.validate().is_err());

    let mut result_digest = exhausted_outcome();
    result_digest.receipt.result_set.sha256 = "0".repeat(64);
    assert!(result_digest.validate().is_err());

    let mut count = exhausted_outcome();
    count.receipt.counts.results += 1;
    assert!(count.validate().is_err());

    let mut final_quality = exhausted_outcome();
    final_quality.receipt.final_quality.usable_result_count += 1;
    assert!(final_quality.validate().is_err());

    let mut decision = exhausted_outcome();
    decision.receipt.executed_tiers[0].decision = SearchTierDecision::Stop;
    assert!(decision.validate().is_err());

    let mut plan = exhausted_outcome();
    plan.receipt.configured_tiers.swap(0, 1);
    assert!(plan.validate().is_err());

    let mut exhaustion = exhausted_outcome();
    exhaustion.receipt.exhausted_below_floor = false;
    assert!(exhaustion.validate().is_err());
}

#[test]
fn result_digest_rejects_same_count_content_and_provenance_substitution() {
    let query = "complete result identity";
    let mut result = SearchResult::new(
        "https://one.example/evidence",
        "Complete evidence",
        "Bounded discovery text",
    )
    .with_engine("engine-0", 1)
    .with_relevance_score(0.8)
    .with_thumbnail("https://one.example/thumb.png")
    .with_published_date("2026-07-29")
    .with_favicon("https://one.example/favicon.ico")
    .with_image(SearchImage::new("https://one.example/image.png").with_description("Figure"));
    result.score = 0.7;
    result.full_text = Some("Fetched source body".to_string());
    result.query_match_score = Some(0.9);

    let mut tier = SearchResults::new();
    tier.add_result(result);
    tier.add_suggestion("related query");
    tier.add_answer("direct answer");
    tier.add_image(
        SearchImage::new("https://one.example/standalone.png")
            .with_description("Standalone figure"),
    );
    tier.add_failure(
        EngineFailure::new("engine-1", "timeout", "bounded failure")
            .with_provider("provider-1")
            .with_transient(true)
            .with_retry_after(3),
    );
    tier.add_report(
        SearchReport::new("engine-0")
            .with_provider("provider-0")
            .with_request_id("request-0")
            .with_total_results(8)
            .with_response_time_ms(20)
            .with_usage(SearchUsage::new().with_credits(1.5))
            .with_metadata("nested", serde_json::json!({"b": [2, 1], "a": true})),
    );
    let mut engine_outcome =
        EngineOutcome::completed("Engine 0", "engine-0", EngineOutcomeKind::Success, 1);
    engine_outcome.provider = Some("provider-0".to_string());
    engine_outcome.duration_ms = 19;
    tier.add_outcome(engine_outcome);
    tier.set_duration(25);

    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor(2));
    cascade.push_tier("tier-0", tier);
    let outcome = cascade
        .finish_with_tier_plan(["tier-0"])
        .expect("rich result receipt");
    outcome.validate().expect("untampered rich result");

    let mutations = [
        (
            "/results/results/0/url",
            serde_json::json!("https://evil.example/replacement"),
        ),
        ("/results/results/0/title", serde_json::json!("Replacement")),
        (
            "/results/results/0/content",
            serde_json::json!("Replacement summary"),
        ),
        ("/results/results/0/score", serde_json::json!(0.6)),
        (
            "/results/results/0/full_text",
            serde_json::json!("Replacement source body"),
        ),
        (
            "/results/suggestions/0",
            serde_json::json!("replacement query"),
        ),
        (
            "/results/answers/0",
            serde_json::json!("replacement answer"),
        ),
        (
            "/results/images/0/url",
            serde_json::json!("https://evil.example/image.png"),
        ),
        (
            "/results/failures/0/message",
            serde_json::json!("replacement failure"),
        ),
        (
            "/results/reports/0/metadata/nested/a",
            serde_json::json!(false),
        ),
        ("/results/outcomes/0/duration_ms", serde_json::json!(20)),
        ("/results/duration_ms", serde_json::json!(26)),
    ];
    for (pointer, replacement) in mutations {
        let mut value = serde_json::to_value(&outcome).expect("serialize rich outcome");
        *value.pointer_mut(pointer).expect("frozen result pointer") = replacement;
        let tampered: SearchCascadeOutcomeV1 =
            serde_json::from_value(value).expect("decode same-shape tampering");
        assert!(
            matches!(
                tampered.validate(),
                Err(SearchCascadeReceiptError::InvalidResultDigest)
            ),
            "result substitution at {pointer} must be rejected"
        );
    }
}

#[test]
fn result_digest_rejects_non_finite_caller_visible_values() {
    let mut tier = SearchResults::new();
    tier.add_report(
        SearchReport::new("engine-0").with_usage(SearchUsage::new().with_credits(f64::NAN)),
    );
    let mut cascade = SearchCascade::new(SearchQuery::new("finite receipt"), floor(1));
    cascade.push_tier("tier-0", tier);

    assert!(matches!(
        cascade.finish_with_tier_plan(["tier-0"]),
        Err(SearchCascadeReceiptError::InvalidResultValue { field })
            if field == "reports[0].usage.credits"
    ));
}

#[test]
fn receipt_rejects_invalid_tier_plans_and_work_after_stop() {
    let duplicate = SearchCascade::new(SearchQuery::new("duplicate"), floor(1))
        .finish_with_tier_plan(["same", "same"]);
    assert!(duplicate.is_err());

    let empty = SearchCascade::new(SearchQuery::new("empty"), floor(1))
        .finish_with_tier_plan(["tier-0", "  "]);
    assert!(empty.is_err());

    let query = "stop then continue";
    let mut cascade = SearchCascade::new(SearchQuery::new(query), floor(1));
    cascade.push_tier(
        "tier-0",
        aggregate(query, "engine-0", &["https://one.example/evidence"]),
    );
    cascade.push_tier("tier-1", SearchResults::new());
    assert!(cascade.finish_with_tier_plan(["tier-0", "tier-1"]).is_err());
}

#[test]
fn receipt_rejects_non_finite_or_impossible_quality_values() {
    let mut floor_value = exhausted_outcome();
    floor_value.receipt.quality_floor.min_query_match = f64::NAN;
    assert!(floor_value.validate().is_err());

    let mut quality = exhausted_outcome();
    quality.receipt.executed_tiers[0]
        .combined_quality
        .mean_query_match = 1.1;
    assert!(quality.validate().is_err());

    let mut empty_quality = exhausted_outcome();
    empty_quality.receipt.executed_tiers[0].combined_quality = SearchQuality {
        mean_query_match: 0.5,
        ..SearchQuality::default()
    };
    assert!(empty_quality.validate().is_err());
}

#[test]
fn receipt_rejects_a_drifted_public_result_count() {
    let mut outcome = exhausted_outcome();
    outcome.results.count += 1;
    assert!(outcome.validate().is_err());
}

#[test]
fn empty_plan_has_explicit_terminal_semantics() {
    let unmet = SearchCascade::new(SearchQuery::new("no tiers"), floor(1))
        .finish_with_tier_plan(std::iter::empty::<&str>())
        .expect("empty plan can be audited");
    assert!(!unmet.receipt.quality_floor_met);
    assert!(unmet.receipt.exhausted_below_floor);

    let met = SearchCascade::new(SearchQuery::new("zero floor"), floor(0))
        .finish_with_tier_plan(std::iter::empty::<&str>())
        .expect("zero floor needs no tier");
    assert!(met.receipt.quality_floor_met);
    assert!(!met.receipt.exhausted_below_floor);
}

#[test]
fn public_receipt_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<SearchQueryBindingV1>();
    assert_send_sync::<SearchResultsBindingV1>();
    assert_send_sync::<SearchCascadeCounts>();
    assert_send_sync::<SearchCascadeReceiptBindingV1>();
    assert_send_sync::<SearchCascadeReceiptV1>();
    assert_send_sync::<SearchCascadeOutcomeV1>();
    assert_send_sync::<SearchCascadeReceiptError>();
}
