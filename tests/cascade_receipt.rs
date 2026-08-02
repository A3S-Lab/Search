use a3s_search::{
    Aggregator, RetrievalRequirements, SearchCascade, SearchCascadeOutcomeV2,
    SearchCascadeReceiptBindingV2, SearchQuery, SearchQueryBindingV1, SearchResult,
    SearchTierDecision, SearchTierDecisionSource, SEARCH_CASCADE_RECEIPT_V2_SCHEMA,
};

#[test]
fn downstream_callers_can_finish_serialize_and_validate_a_lazy_cascade() {
    let query = "portable public receipt";
    let results = Aggregator::new().aggregate(vec![(
        "generic-engine".to_string(),
        vec![SearchResult::new(
            "https://example.test/portable-receipt",
            "opaque title",
            "opaque snippet",
        )],
    )]);
    let mut cascade =
        SearchCascade::new(SearchQuery::new(query), RetrievalRequirements::for_limit(1));
    assert_eq!(
        cascade.push_tier("caller-tier", results),
        SearchTierDecision::Stop
    );

    let outcome = cascade
        .finish_with_tier_plan(["caller-tier", "unused-tier"])
        .expect("public cascade should finish");
    assert_eq!(outcome.receipt.schema, SEARCH_CASCADE_RECEIPT_V2_SCHEMA);
    assert_eq!(outcome.receipt.executed_tiers.len(), 1);
    assert_eq!(
        outcome.receipt.executed_tiers[0].decision_source,
        SearchTierDecisionSource::RetrievalRequirements
    );
    assert!(outcome.receipt.retrieval_requirements_met);
    assert!(!outcome.receipt.exhausted);
    assert_eq!(outcome.receipt.result_set.sha256.len(), 64);
    let receipt_binding: SearchCascadeReceiptBindingV2 = outcome
        .receipt_binding()
        .expect("bind complete public receipt");
    receipt_binding
        .validate(&outcome.receipt)
        .expect("validate complete public receipt binding");

    let encoded = serde_json::to_vec(&outcome).expect("encode public outcome");
    let decoded: SearchCascadeOutcomeV2 =
        serde_json::from_slice(&encoded).expect("decode public outcome");
    assert_eq!(
        outcome.results.items()[0].score.to_bits(),
        decoded.results.items()[0].score.to_bits(),
        "caller-visible rank-fusion score must be bit-stable across JSON"
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
