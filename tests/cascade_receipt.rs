use a3s_search::{
    Aggregator, SearchCascade, SearchCascadeOutcomeV1, SearchCascadeReceiptBindingV1,
    SearchQualityFloor, SearchQuery, SearchQueryBindingV1, SearchResult, SearchTierDecision,
    SEARCH_CASCADE_RECEIPT_V1_SCHEMA,
};

#[test]
fn downstream_callers_can_finish_serialize_and_validate_a_lazy_cascade() {
    let query = "portable public receipt";
    let results = Aggregator::new().aggregate_for_query(
        query,
        vec![(
            "generic-engine".to_string(),
            vec![SearchResult::new(
                "https://example.test/portable-receipt",
                query,
                "Portable public receipt evidence",
            )],
        )],
    );
    let mut cascade = SearchCascade::new(SearchQuery::new(query), SearchQualityFloor::for_limit(1));
    assert_eq!(
        cascade.push_tier("caller-tier", results),
        SearchTierDecision::Stop
    );

    let outcome = cascade
        .finish_with_tier_plan(["caller-tier", "unused-tier"])
        .expect("public cascade should finish");
    assert_eq!(outcome.receipt.schema, SEARCH_CASCADE_RECEIPT_V1_SCHEMA);
    assert_eq!(outcome.receipt.executed_tiers.len(), 1);
    assert!(outcome.receipt.quality_floor_met);
    assert!(!outcome.receipt.exhausted_below_floor);
    assert_eq!(outcome.receipt.result_set.sha256.len(), 64);
    let receipt_binding: SearchCascadeReceiptBindingV1 = outcome
        .receipt_binding()
        .expect("bind complete public receipt");
    receipt_binding
        .validate(&outcome.receipt)
        .expect("validate complete public receipt binding");

    let encoded = serde_json::to_vec(&outcome).expect("encode public outcome");
    let decoded: SearchCascadeOutcomeV1 =
        serde_json::from_slice(&encoded).expect("decode public outcome");
    assert_eq!(
        outcome.results.items()[0].score.to_bits(),
        decoded.results.items()[0].score.to_bits(),
        "caller-visible ranking score must be bit-stable across JSON"
    );
    assert_eq!(
        outcome.results.items()[0]
            .query_match_score
            .expect("query alignment")
            .to_bits(),
        decoded.results.items()[0]
            .query_match_score
            .expect("decoded query alignment")
            .to_bits(),
        "caller-visible query alignment must be bit-stable across JSON"
    );
    decoded.validate().expect("validate public outcome");

    let query_binding = SearchQueryBindingV1::new(SearchQuery::new(query));
    query_binding
        .validate()
        .expect("validate public query binding");
    assert_eq!(query_binding.sha256, decoded.receipt.query.sha256);

    let mut substituted = decoded;
    substituted.results.items_mut()[0].content = "same-count replacement".to_string();
    assert!(substituted.validate().is_err());
}
