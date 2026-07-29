//! Frozen V1 identity for complete search cascade receipts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::query_binding::encode_search_query_v1;
use super::{
    is_canonical_sha256, SearchCascadeCounts, SearchCascadeReceiptError, SearchCascadeReceiptV1,
    SearchQuality, SearchQualityFloor, SearchQueryBindingV1, SearchResultsBindingV1,
    SearchTierDecision, SearchTierReport,
};

const SEARCH_CASCADE_RECEIPT_BINDING_V1_DOMAIN: &[u8] = b"a3s/search-cascade-receipt-binding/v1\0";

/// Canonical SHA-256 identity of every field in a V1 cascade receipt.
///
/// This binding detects substitution when compared with a trusted expected
/// digest. It is not a signature and does not prove execution, plan
/// precommitment, or the identity of the producer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SearchCascadeReceiptBindingV1 {
    /// Lowercase hexadecimal SHA-256 over the frozen V1 receipt encoding.
    pub sha256: String,
}

impl SearchCascadeReceiptBindingV1 {
    /// Computes the canonical identity of a structurally valid V1 receipt.
    pub fn new(receipt: &SearchCascadeReceiptV1) -> Result<Self, SearchCascadeReceiptError> {
        receipt.validate_internal()?;
        Ok(Self {
            sha256: search_cascade_receipt_sha256(receipt)?,
        })
    }

    /// Recomputes and validates this complete-receipt identity.
    pub fn validate(
        &self,
        receipt: &SearchCascadeReceiptV1,
    ) -> Result<(), SearchCascadeReceiptError> {
        receipt.validate_internal()?;
        if !is_canonical_sha256(&self.sha256)
            || self.sha256 != search_cascade_receipt_sha256(receipt)?
        {
            return Err(SearchCascadeReceiptError::InvalidReceiptDigest);
        }
        Ok(())
    }
}

fn search_cascade_receipt_sha256(
    receipt: &SearchCascadeReceiptV1,
) -> Result<String, SearchCascadeReceiptError> {
    let SearchCascadeReceiptV1 {
        schema,
        query,
        quality_floor,
        final_quality,
        result_set,
        configured_tiers,
        executed_tiers,
        quality_floor_met,
        exhausted_below_floor,
        counts,
    } = receipt;
    let mut encoder = ReceiptEncoder::new();

    encoder.label("schema");
    encoder.string(schema);

    encoder.label("query");
    encode_query_binding(&mut encoder, query);

    encoder.label("quality_floor");
    encode_quality_floor(&mut encoder, quality_floor)?;

    encoder.label("final_quality");
    encode_quality(&mut encoder, final_quality, "final_quality")?;

    encoder.label("result_set");
    encode_result_binding(&mut encoder, result_set);

    encoder.label("configured_tiers");
    encoder.strings(configured_tiers);

    encoder.label("executed_tiers");
    encoder.length(executed_tiers.len());
    for (index, report) in executed_tiers.iter().enumerate() {
        encode_tier_report(&mut encoder, report, index)?;
    }

    encoder.label("quality_floor_met");
    encoder.boolean(*quality_floor_met);
    encoder.label("exhausted_below_floor");
    encoder.boolean(*exhausted_below_floor);

    encoder.label("counts");
    encode_counts(&mut encoder, counts);

    Ok(encoder.finish())
}

fn encode_query_binding(encoder: &mut ReceiptEncoder, binding: &SearchQueryBindingV1) {
    let SearchQueryBindingV1 { sha256, value } = binding;
    encoder.label("sha256");
    encoder.string(sha256);
    encoder.label("value");
    encode_search_query_v1(&mut encoder.hasher, value);
}

fn encode_quality_floor(
    encoder: &mut ReceiptEncoder,
    floor: &SearchQualityFloor,
) -> Result<(), SearchCascadeReceiptError> {
    let SearchQualityFloor {
        min_usable_results,
        min_unique_hosts,
        min_contributing_engines,
        min_aligned_results,
        min_consensus_results,
        min_query_match,
        min_mean_query_match,
    } = floor;
    encoder.label("min_usable_results");
    encoder.length(*min_usable_results);
    encoder.label("min_unique_hosts");
    encoder.length(*min_unique_hosts);
    encoder.label("min_contributing_engines");
    encoder.length(*min_contributing_engines);
    encoder.label("min_aligned_results");
    encoder.length(*min_aligned_results);
    encoder.label("min_consensus_results");
    encoder.length(*min_consensus_results);
    encoder.label("min_query_match");
    encoder.f64(*min_query_match, "quality_floor.min_query_match")?;
    encoder.label("min_mean_query_match");
    encoder.f64(*min_mean_query_match, "quality_floor.min_mean_query_match")
}

fn encode_quality(
    encoder: &mut ReceiptEncoder,
    quality: &SearchQuality,
    path: &str,
) -> Result<(), SearchCascadeReceiptError> {
    let SearchQuality {
        usable_result_count,
        unique_host_count,
        contributing_engine_count,
        consensus_result_count,
        aligned_result_count,
        mean_query_match,
    } = quality;
    encoder.label("usable_result_count");
    encoder.length(*usable_result_count);
    encoder.label("unique_host_count");
    encoder.length(*unique_host_count);
    encoder.label("contributing_engine_count");
    encoder.length(*contributing_engine_count);
    encoder.label("consensus_result_count");
    encoder.length(*consensus_result_count);
    encoder.label("aligned_result_count");
    encoder.length(*aligned_result_count);
    encoder.label("mean_query_match");
    encoder.f64(*mean_query_match, &format!("{path}.mean_query_match"))
}

fn encode_result_binding(encoder: &mut ReceiptEncoder, binding: &SearchResultsBindingV1) {
    let SearchResultsBindingV1 { sha256 } = binding;
    encoder.label("sha256");
    encoder.string(sha256);
}

fn encode_tier_report(
    encoder: &mut ReceiptEncoder,
    report: &SearchTierReport,
    index: usize,
) -> Result<(), SearchCascadeReceiptError> {
    let SearchTierReport {
        tier,
        combined_quality,
        decision,
    } = report;
    encoder.label("tier");
    encoder.string(tier);
    encoder.label("combined_quality");
    encode_quality(
        encoder,
        combined_quality,
        &format!("executed_tiers[{index}].combined_quality"),
    )?;
    encoder.label("decision");
    encoder.tag(match decision {
        SearchTierDecision::Stop => 0,
        SearchTierDecision::Continue => 1,
    });
    Ok(())
}

fn encode_counts(encoder: &mut ReceiptEncoder, counts: &SearchCascadeCounts) {
    let SearchCascadeCounts {
        results,
        failures,
        outcomes,
    } = counts;
    encoder.label("results");
    encoder.length(*results);
    encoder.label("failures");
    encoder.length(*failures);
    encoder.label("outcomes");
    encoder.length(*outcomes);
}

struct ReceiptEncoder {
    hasher: Sha256,
}

impl ReceiptEncoder {
    fn new() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(SEARCH_CASCADE_RECEIPT_BINDING_V1_DOMAIN);
        Self { hasher }
    }

    fn finish(self) -> String {
        format!("{:x}", self.hasher.finalize())
    }

    fn label(&mut self, value: &str) {
        self.string(value);
    }

    fn string(&mut self, value: &str) {
        self.length(value.len());
        self.hasher.update(value.as_bytes());
    }

    fn strings(&mut self, values: &[String]) {
        self.length(values.len());
        for value in values {
            self.string(value);
        }
    }

    fn length(&mut self, value: usize) {
        self.hasher.update((value as u128).to_be_bytes());
    }

    fn boolean(&mut self, value: bool) {
        self.tag(u8::from(value));
    }

    fn tag(&mut self, value: u8) {
        self.hasher.update([value]);
    }

    fn f64(&mut self, value: f64, field: &str) -> Result<(), SearchCascadeReceiptError> {
        if !value.is_finite() {
            return Err(SearchCascadeReceiptError::InvalidQualityValue {
                field: field.to_string(),
            });
        }
        let normalized = if value == 0.0 { 0.0 } else { value };
        self.hasher.update(normalized.to_bits().to_be_bytes());
        Ok(())
    }
}
