//! Versioned, structurally validated records for caller-defined search cascades.

use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use super::{
    SearchCascade, SearchQuality, SearchQualityFloor, SearchTierDecision, SearchTierReport,
};
use crate::SearchResults;

mod query_binding;
mod receipt_binding;
mod result_binding;
mod wire;

pub use query_binding::SearchQueryBindingV1;
pub use receipt_binding::SearchCascadeReceiptBindingV1;
pub use result_binding::SearchResultsBindingV1;

/// Stable schema identifier for [`SearchCascadeReceiptV1`].
pub const SEARCH_CASCADE_RECEIPT_V1_SCHEMA: &str = "a3s/search-cascade-receipt/v1";

/// Counts that bind a cascade receipt to its returned result container.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SearchCascadeCounts {
    /// Canonically merged ordinary result count.
    pub results: usize,
    /// Structured engine failure count.
    pub failures: usize,
    /// Typed engine outcome count.
    pub outcomes: usize,
}

/// Version-one audit record for one caller-defined lazy tier cascade.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SearchCascadeReceiptV1 {
    /// Exact receipt schema identifier.
    pub schema: String,
    /// Query and complete typed query identity.
    pub query: SearchQueryBindingV1,
    /// Caller-selected generic quality floor.
    pub quality_floor: SearchQualityFloor,
    /// Quality of the final canonically merged result set.
    pub final_quality: SearchQuality,
    /// Deterministic identity of every caller-visible final result field.
    pub result_set: SearchResultsBindingV1,
    /// Ordered opaque identifiers for every available tier.
    pub configured_tiers: Vec<String>,
    /// Ordered reports for tiers the caller records as executed.
    pub executed_tiers: Vec<SearchTierReport>,
    /// Whether final quality satisfies `quality_floor`.
    pub quality_floor_met: bool,
    /// Whether every tier in the caller-declared plan is reported as executed
    /// without satisfying the floor.
    pub exhausted_below_floor: bool,
    /// Counts derived from the returned result container.
    pub counts: SearchCascadeCounts,
}

impl SearchCascadeReceiptV1 {
    /// Validates the receipt against its returned results.
    ///
    /// This proves structural self-consistency. Authenticity still requires an
    /// external trusted signature, digest log, or equivalent authority.
    pub fn validate(&self, results: &SearchResults) -> Result<(), SearchCascadeReceiptError> {
        self.validate_internal()?;
        validate_result_counts(&self.counts, results)?;
        self.result_set.validate(results)?;
        if self.executed_tiers.is_empty() && !is_initial_empty_results(results) {
            return Err(SearchCascadeReceiptError::OutputWithoutExecutedTier);
        }

        let recomputed = SearchQuality::evaluate(
            &self.query.value.query,
            results,
            self.quality_floor.min_query_match,
        );
        if self.final_quality != recomputed {
            return Err(SearchCascadeReceiptError::FinalQualityMismatch);
        }

        Ok(())
    }

    fn validate_internal(&self) -> Result<(), SearchCascadeReceiptError> {
        if self.schema != SEARCH_CASCADE_RECEIPT_V1_SCHEMA {
            return Err(SearchCascadeReceiptError::UnsupportedSchema {
                actual: self.schema.clone(),
            });
        }
        self.query.validate()?;
        validate_floor(&self.quality_floor)?;
        validate_tier_plan(&self.configured_tiers)?;

        if self.executed_tiers.len() > self.configured_tiers.len() {
            return Err(SearchCascadeReceiptError::TierPlanMismatch {
                index: self.configured_tiers.len(),
            });
        }
        for (index, report) in self.executed_tiers.iter().enumerate() {
            if self.configured_tiers.get(index) != Some(&report.tier) {
                return Err(SearchCascadeReceiptError::TierPlanMismatch { index });
            }
            validate_quality(
                &report.combined_quality,
                &format!("executed_tiers[{index}]"),
            )?;
            let expected = if self.quality_floor.is_met(&report.combined_quality) {
                SearchTierDecision::Stop
            } else {
                SearchTierDecision::Continue
            };
            if report.decision != expected {
                return Err(SearchCascadeReceiptError::InvalidTierDecision { index });
            }
            if report.decision == SearchTierDecision::Stop && index + 1 != self.executed_tiers.len()
            {
                return Err(SearchCascadeReceiptError::TierExecutedAfterStop { index: index + 1 });
            }
        }

        if !is_canonical_sha256(&self.result_set.sha256) {
            return Err(SearchCascadeReceiptError::InvalidResultDigest);
        }

        validate_quality(&self.final_quality, "final_quality")?;
        if let Some(last) = self.executed_tiers.last() {
            if last.combined_quality != self.final_quality {
                return Err(SearchCascadeReceiptError::FinalTierQualityMismatch);
            }
        }

        let quality_floor_met = self.quality_floor.is_met(&self.final_quality);
        if self.quality_floor_met != quality_floor_met {
            return Err(SearchCascadeReceiptError::QualityFloorStateMismatch);
        }
        let exhausted_below_floor =
            !quality_floor_met && self.executed_tiers.len() == self.configured_tiers.len();
        if self.exhausted_below_floor != exhausted_below_floor {
            return Err(SearchCascadeReceiptError::ExhaustionStateMismatch);
        }

        Ok(())
    }
}

impl Serialize for SearchCascadeReceiptV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        wire::serialize_receipt(self, serializer)
    }
}

impl<'de> Deserialize<'de> for SearchCascadeReceiptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        wire::deserialize_receipt(deserializer)
    }
}

/// Final merged results paired with their version-one cascade receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SearchCascadeOutcomeV1 {
    /// Internally self-consistent cascade receipt.
    pub receipt: SearchCascadeReceiptV1,
    /// Canonically merged search output.
    pub results: SearchResults,
}

impl SearchCascadeOutcomeV1 {
    /// Validates the receipt and returned results as one outcome.
    pub fn validate(&self) -> Result<(), SearchCascadeReceiptError> {
        self.receipt.validate(&self.results)
    }

    /// Returns the canonical identity of the complete validated receipt.
    ///
    /// The digest detects receipt substitution when compared with a trusted
    /// expected value. It does not prove execution, precommitment, or signer
    /// authenticity by itself.
    pub fn receipt_binding(
        &self,
    ) -> Result<SearchCascadeReceiptBindingV1, SearchCascadeReceiptError> {
        self.validate()?;
        SearchCascadeReceiptBindingV1::new(&self.receipt)
    }
}

/// Validation failure for a versioned search cascade receipt.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SearchCascadeReceiptError {
    /// Receipt schema is not version one.
    #[error("unsupported search cascade receipt schema: {actual}")]
    UnsupportedSchema { actual: String },
    /// Query digest is malformed or does not bind the typed query.
    #[error("search cascade receipt query digest is invalid")]
    InvalidQueryDigest,
    /// Result-set digest is malformed or does not bind the returned results.
    #[error("search cascade receipt result-set digest is invalid")]
    InvalidResultDigest,
    /// Complete-receipt digest is malformed or does not bind the receipt.
    #[error("search cascade complete-receipt digest is invalid")]
    InvalidReceiptDigest,
    /// A result field cannot be represented by the frozen digest encoding.
    #[error("search cascade result set has an invalid value at {field}")]
    InvalidResultValue { field: String },
    /// A floating-point quality or floor value is non-finite or impossible.
    #[error("search cascade receipt has an invalid quality value at {field}")]
    InvalidQualityValue { field: String },
    /// Configured tier identifiers are empty or repeated.
    #[error("search cascade receipt has an invalid tier plan at index {index}: {reason}")]
    InvalidTierPlan { index: usize, reason: &'static str },
    /// Executed tiers are not the exact ordered prefix of configured tiers.
    #[error("search cascade executed tier does not match its plan at index {index}")]
    TierPlanMismatch { index: usize },
    /// A tier decision does not agree with its recorded combined quality.
    #[error("search cascade tier decision is invalid at index {index}")]
    InvalidTierDecision { index: usize },
    /// Work is recorded after an earlier tier stopped the cascade.
    #[error("search cascade executed tier {index} after an earlier stop decision")]
    TierExecutedAfterStop { index: usize },
    /// Public `SearchResults::count` disagrees with the actual result vector.
    #[error("search result container count is {declared}, but contains {actual} results")]
    ResultContainerCountMismatch { declared: usize, actual: usize },
    /// Receipt count disagrees with the returned output.
    #[error("search cascade {field} count is {declared}, but output contains {actual}")]
    ReceiptCountMismatch {
        field: &'static str,
        declared: usize,
        actual: usize,
    },
    /// Output exists even though no tier was recorded as executed.
    #[error("search cascade returned output without an executed tier")]
    OutputWithoutExecutedTier,
    /// Final quality cannot be recomputed from the returned output.
    #[error("search cascade final quality does not match returned results")]
    FinalQualityMismatch,
    /// Last tier quality differs from final quality.
    #[error("search cascade final tier quality does not match final quality")]
    FinalTierQualityMismatch,
    /// `quality_floor_met` does not match the floor evaluation.
    #[error("search cascade quality-floor state is inconsistent")]
    QualityFloorStateMismatch,
    /// `exhausted_below_floor` does not match plan execution and final quality.
    #[error("search cascade exhaustion state is inconsistent")]
    ExhaustionStateMismatch,
}

impl SearchCascade {
    /// Consumes the cascade and returns results with a validated V1 receipt.
    ///
    /// Tier identifiers are caller-defined opaque values. Executed tiers must
    /// be the exact ordered prefix of this configured plan. A below-floor
    /// prefix shorter than the plan remains valid and explicitly represents an
    /// interrupted or otherwise incomplete caller-owned cascade.
    pub fn finish_with_tier_plan<I, S>(
        self,
        configured_tiers: I,
    ) -> Result<SearchCascadeOutcomeV1, SearchCascadeReceiptError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let configured_tiers = configured_tiers
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let final_quality = self.quality();
        let quality_floor_met = self.floor.is_met(&final_quality);
        let exhausted_below_floor =
            !quality_floor_met && self.reports.len() == configured_tiers.len();
        let counts = counts_for_results(&self.results);
        let result_set = SearchResultsBindingV1::new(&self.results)?;
        let receipt = SearchCascadeReceiptV1 {
            schema: SEARCH_CASCADE_RECEIPT_V1_SCHEMA.to_string(),
            query: SearchQueryBindingV1::new(self.query),
            quality_floor: self.floor,
            final_quality,
            result_set,
            configured_tiers,
            executed_tiers: self.reports,
            quality_floor_met,
            exhausted_below_floor,
            counts,
        };
        let outcome = SearchCascadeOutcomeV1 {
            receipt,
            results: self.results,
        };
        outcome.validate()?;
        Ok(outcome)
    }
}

fn validate_floor(floor: &SearchQualityFloor) -> Result<(), SearchCascadeReceiptError> {
    for (field, value) in [
        ("quality_floor.min_query_match", floor.min_query_match),
        (
            "quality_floor.min_mean_query_match",
            floor.min_mean_query_match,
        ),
    ] {
        if !value.is_finite() {
            return Err(SearchCascadeReceiptError::InvalidQualityValue {
                field: field.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_quality(quality: &SearchQuality, path: &str) -> Result<(), SearchCascadeReceiptError> {
    let mean_is_valid =
        quality.mean_query_match.is_finite() && (0.0..=1.0).contains(&quality.mean_query_match);
    let counts_are_possible = quality.unique_host_count <= quality.usable_result_count
        && quality.consensus_result_count <= quality.usable_result_count
        && quality.aligned_result_count <= quality.usable_result_count;
    let empty_state_is_consistent = if quality.usable_result_count == 0 {
        quality.contributing_engine_count == 0 && quality.mean_query_match == 0.0
    } else {
        quality.unique_host_count > 0
    };
    if !mean_is_valid || !counts_are_possible || !empty_state_is_consistent {
        return Err(SearchCascadeReceiptError::InvalidQualityValue {
            field: path.to_string(),
        });
    }
    Ok(())
}

fn validate_tier_plan(tiers: &[String]) -> Result<(), SearchCascadeReceiptError> {
    let mut seen = HashSet::new();
    for (index, tier) in tiers.iter().enumerate() {
        if tier.trim().is_empty() {
            return Err(SearchCascadeReceiptError::InvalidTierPlan {
                index,
                reason: "tier identifier is empty",
            });
        }
        if !seen.insert(tier.as_str()) {
            return Err(SearchCascadeReceiptError::InvalidTierPlan {
                index,
                reason: "tier identifier is duplicated",
            });
        }
    }
    Ok(())
}

fn validate_result_counts(
    counts: &SearchCascadeCounts,
    results: &SearchResults,
) -> Result<(), SearchCascadeReceiptError> {
    if results.count != results.items().len() {
        return Err(SearchCascadeReceiptError::ResultContainerCountMismatch {
            declared: results.count,
            actual: results.items().len(),
        });
    }
    for (field, declared, actual) in [
        ("results", counts.results, results.items().len()),
        ("failures", counts.failures, results.failures().len()),
        ("outcomes", counts.outcomes, results.outcomes().len()),
    ] {
        if declared != actual {
            return Err(SearchCascadeReceiptError::ReceiptCountMismatch {
                field,
                declared,
                actual,
            });
        }
    }
    Ok(())
}

fn counts_for_results(results: &SearchResults) -> SearchCascadeCounts {
    SearchCascadeCounts {
        results: results.items().len(),
        failures: results.failures().len(),
        outcomes: results.outcomes().len(),
    }
}

fn is_initial_empty_results(results: &SearchResults) -> bool {
    results.items().is_empty()
        && results.suggestions().is_empty()
        && results.answers().is_empty()
        && results.images().is_empty()
        && results.errors().is_empty()
        && results.failures().is_empty()
        && results.reports().is_empty()
        && results.outcomes().is_empty()
        && results.count == 0
        && results.duration_ms == 0
}

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
