use super::receipt_tests::exhausted_outcome;
use super::*;

#[test]
fn complete_receipt_binding_has_a_frozen_domain_separated_digest() {
    let outcome = exhausted_outcome();
    let binding = outcome
        .receipt_binding()
        .expect("bind complete validated receipt");

    assert_eq!(
        binding.sha256,
        "9aab7ef48bef6a2eda16baca1359d5c8f4abda52bddde9f446d335205f057e45"
    );
    binding
        .validate(&outcome.receipt)
        .expect("complete receipt digest validates");
}

#[test]
fn complete_receipt_binding_rejects_coherent_same_output_substitutions() {
    let outcome = exhausted_outcome();
    let binding = outcome
        .receipt_binding()
        .expect("bind original complete receipt");
    let original_results = serde_json::to_value(&outcome.results).expect("serialize result set");

    let mut renamed_plan = outcome.clone();
    for (index, tier) in renamed_plan.receipt.configured_tiers.iter_mut().enumerate() {
        *tier = format!("replacement-tier-{index}");
        renamed_plan.receipt.executed_tiers[index].tier = tier.clone();
    }

    let mut incomplete_plan = outcome.clone();
    incomplete_plan
        .receipt
        .configured_tiers
        .push("additional-tier".to_string());
    incomplete_plan.receipt.exhausted_below_floor = false;

    let mut earlier_quality = outcome.clone();
    earlier_quality.receipt.executed_tiers[0]
        .combined_quality
        .aligned_result_count = 0;

    let mut different_floor = outcome.clone();
    different_floor.receipt.quality_floor.min_mean_query_match = 0.5;

    let mut different_typed_query = outcome.clone();
    let mut query = different_typed_query.receipt.query.value.clone();
    query.page = 2;
    different_typed_query.receipt.query = SearchQueryBindingV1::new(query);

    for substituted in [
        renamed_plan,
        incomplete_plan,
        earlier_quality,
        different_floor,
        different_typed_query,
    ] {
        assert_eq!(
            serde_json::to_value(&substituted.results).expect("serialize substituted result set"),
            original_results
        );
        substituted
            .validate()
            .expect("coherent substituted receipt remains structurally valid");
        assert!(matches!(
            binding.validate(&substituted.receipt),
            Err(SearchCascadeReceiptError::InvalidReceiptDigest)
        ));
    }
}

#[test]
fn complete_receipt_binding_normalizes_signed_zero_and_rejects_non_finite_floats() {
    let positive_zero = exhausted_outcome();
    let positive_binding = positive_zero
        .receipt_binding()
        .expect("bind positive-zero receipt");

    let mut negative_zero = positive_zero.clone();
    negative_zero.receipt.quality_floor.min_mean_query_match = -0.0;
    negative_zero
        .validate()
        .expect("signed zero has the same floor semantics");
    let negative_binding = negative_zero
        .receipt_binding()
        .expect("bind negative-zero receipt");
    assert_eq!(positive_binding, negative_binding);

    let mut non_finite = positive_zero.receipt;
    non_finite.quality_floor.min_mean_query_match = f64::INFINITY;
    assert!(matches!(
        SearchCascadeReceiptBindingV1::new(&non_finite),
        Err(SearchCascadeReceiptError::InvalidQualityValue { field })
            if field == "quality_floor.min_mean_query_match"
    ));
}
