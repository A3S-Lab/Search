use super::*;
use crate::SearchResult;

fn outcome() -> SearchCascadeOutcomeV2 {
    let query = SearchQuery::new("portable query");
    let mut cascade = SearchCascade::new(query, RetrievalRequirements::for_limit(1));
    let mut results = SearchResults::new();
    results.add_result(
        SearchResult::new("https://example.com/report", "title", "snippet")
            .with_engine("fixture", 1),
    );
    cascade.push_tier("http", results);
    cascade.finish_with_tier_plan(["http", "headless"]).unwrap()
}

#[test]
fn v2_receipt_round_trips_and_validates() {
    let outcome = outcome();
    let encoded = serde_json::to_vec(&outcome).unwrap();
    let decoded: SearchCascadeOutcomeV2 = serde_json::from_slice(&encoded).unwrap();

    decoded.validate().unwrap();
    assert_eq!(decoded.receipt.schema, SEARCH_CASCADE_RECEIPT_V2_SCHEMA);
    assert!(decoded.receipt.retrieval_requirements_met);
    assert!(!decoded.receipt.exhausted);
    assert_eq!(decoded.receipt_binding().unwrap().sha256.len(), 64);
}

#[test]
fn receipt_rejects_tampered_results() {
    let mut outcome = outcome();
    outcome.results.items_mut()[0].url = "https://substituted.example/report".to_string();

    assert_eq!(
        outcome.validate(),
        Err(SearchCascadeReceiptError::InvalidResultDigest)
    );
}

#[test]
fn external_decisions_are_attributed_without_semantic_revalidation() {
    let mut cascade = SearchCascade::new(
        SearchQuery::new("portable query"),
        RetrievalRequirements::for_limit(5),
    );
    let mut results = SearchResults::new();
    results.add_result(
        SearchResult::new("https://example.com/report", "title", "snippet")
            .with_engine("fixture", 1),
    );
    cascade.push_tier_with_decision("external", results, SearchTierDecision::Stop);
    let outcome = cascade.finish_with_tier_plan(["external", "next"]).unwrap();

    outcome.validate().unwrap();
    assert!(!outcome.receipt.retrieval_requirements_met);
    assert!(!outcome.receipt.exhausted);
    assert_eq!(
        outcome.receipt.executed_tiers[0].decision_source,
        SearchTierDecisionSource::ExternalPolicy
    );
}

#[test]
fn public_receipt_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SearchCascadeCounts>();
    assert_send_sync::<SearchCascadeReceiptBindingV2>();
    assert_send_sync::<SearchCascadeReceiptV2>();
    assert_send_sync::<SearchCascadeOutcomeV2>();
    assert_send_sync::<SearchCascadeReceiptError>();
}
