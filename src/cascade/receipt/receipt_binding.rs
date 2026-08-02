//! Frozen V2 identity for complete search cascade receipts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::query_binding::encode_search_query_v1;
use super::{
    is_canonical_sha256, SearchCascadeCounts, SearchCascadeReceiptError, SearchCascadeReceiptV2,
    SearchQueryBindingV1, SearchResultsBindingV2,
};
use crate::{
    RetrievalHealth, RetrievalRequirements, SearchTierDecision, SearchTierDecisionSource,
    SearchTierReport,
};

const SEARCH_CASCADE_RECEIPT_BINDING_V2_DOMAIN: &[u8] = b"a3s/search-cascade-receipt-binding/v2\0";

/// Canonical SHA-256 identity of every field in a V2 cascade receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SearchCascadeReceiptBindingV2 {
    /// Lowercase hexadecimal SHA-256 over the frozen V2 receipt encoding.
    pub sha256: String,
}

impl SearchCascadeReceiptBindingV2 {
    /// Computes the canonical identity of a structurally valid V2 receipt.
    pub fn new(receipt: &SearchCascadeReceiptV2) -> Result<Self, SearchCascadeReceiptError> {
        receipt.validate_internal()?;
        Ok(Self {
            sha256: search_cascade_receipt_sha256(receipt),
        })
    }

    /// Recomputes and validates this complete-receipt identity.
    pub fn validate(
        &self,
        receipt: &SearchCascadeReceiptV2,
    ) -> Result<(), SearchCascadeReceiptError> {
        receipt.validate_internal()?;
        if !is_canonical_sha256(&self.sha256)
            || self.sha256 != search_cascade_receipt_sha256(receipt)
        {
            return Err(SearchCascadeReceiptError::InvalidReceiptDigest);
        }
        Ok(())
    }
}

fn search_cascade_receipt_sha256(receipt: &SearchCascadeReceiptV2) -> String {
    let SearchCascadeReceiptV2 {
        schema,
        query,
        retrieval_requirements,
        final_health,
        result_set,
        configured_tiers,
        executed_tiers,
        retrieval_requirements_met,
        exhausted,
        counts,
    } = receipt;
    let mut encoder = ReceiptEncoder::new();

    encoder.label("schema");
    encoder.string(schema);
    encoder.label("query");
    encode_query_binding(&mut encoder, query);
    encoder.label("retrieval_requirements");
    encode_requirements(&mut encoder, retrieval_requirements);
    encoder.label("final_health");
    encode_health(&mut encoder, final_health);
    encoder.label("result_set");
    encode_result_binding(&mut encoder, result_set);
    encoder.label("configured_tiers");
    encoder.strings(configured_tiers);
    encoder.label("executed_tiers");
    encoder.length(executed_tiers.len());
    for report in executed_tiers {
        encode_tier_report(&mut encoder, report);
    }
    encoder.label("retrieval_requirements_met");
    encoder.boolean(*retrieval_requirements_met);
    encoder.label("exhausted");
    encoder.boolean(*exhausted);
    encoder.label("counts");
    encode_counts(&mut encoder, counts);

    encoder.finish()
}

fn encode_query_binding(encoder: &mut ReceiptEncoder, binding: &SearchQueryBindingV1) {
    let SearchQueryBindingV1 { sha256, value } = binding;
    encoder.label("sha256");
    encoder.string(sha256);
    encoder.label("value");
    encode_search_query_v1(&mut encoder.hasher, value);
}

fn encode_requirements(encoder: &mut ReceiptEncoder, requirements: &RetrievalRequirements) {
    let RetrievalRequirements {
        min_usable_results,
        min_unique_hosts,
        min_contributing_engines,
        min_consensus_results,
    } = requirements;
    encoder.label("min_usable_results");
    encoder.length(*min_usable_results);
    encoder.label("min_unique_hosts");
    encoder.length(*min_unique_hosts);
    encoder.label("min_contributing_engines");
    encoder.length(*min_contributing_engines);
    encoder.label("min_consensus_results");
    encoder.length(*min_consensus_results);
}

fn encode_health(encoder: &mut ReceiptEncoder, health: &RetrievalHealth) {
    let RetrievalHealth {
        usable_result_count,
        invalid_result_count,
        unique_host_count,
        contributing_engine_count,
        consensus_result_count,
        attempted_engine_count,
        successful_engine_count,
        empty_engine_count,
        failed_engine_count,
        timed_out_engine_count,
        rejected_engine_count,
        circuit_open_engine_count,
    } = health;
    for (label, value) in [
        ("usable_result_count", *usable_result_count),
        ("invalid_result_count", *invalid_result_count),
        ("unique_host_count", *unique_host_count),
        ("contributing_engine_count", *contributing_engine_count),
        ("consensus_result_count", *consensus_result_count),
        ("attempted_engine_count", *attempted_engine_count),
        ("successful_engine_count", *successful_engine_count),
        ("empty_engine_count", *empty_engine_count),
        ("failed_engine_count", *failed_engine_count),
        ("timed_out_engine_count", *timed_out_engine_count),
        ("rejected_engine_count", *rejected_engine_count),
        ("circuit_open_engine_count", *circuit_open_engine_count),
    ] {
        encoder.label(label);
        encoder.length(value);
    }
}

fn encode_result_binding(encoder: &mut ReceiptEncoder, binding: &SearchResultsBindingV2) {
    let SearchResultsBindingV2 { sha256 } = binding;
    encoder.label("sha256");
    encoder.string(sha256);
}

fn encode_tier_report(encoder: &mut ReceiptEncoder, report: &SearchTierReport) {
    let SearchTierReport {
        tier,
        combined_health,
        decision,
        decision_source,
    } = report;
    encoder.label("tier");
    encoder.string(tier);
    encoder.label("combined_health");
    encode_health(encoder, combined_health);
    encoder.label("decision");
    encoder.tag(match decision {
        SearchTierDecision::Stop => 0,
        SearchTierDecision::Continue => 1,
    });
    encoder.label("decision_source");
    encoder.tag(match decision_source {
        SearchTierDecisionSource::RetrievalRequirements => 0,
        SearchTierDecisionSource::ExternalPolicy => 1,
    });
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
        hasher.update(SEARCH_CASCADE_RECEIPT_BINDING_V2_DOMAIN);
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
}
