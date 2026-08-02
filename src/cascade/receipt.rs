//! Versioned, structurally validated records for caller-defined search cascades.

use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use super::{
    RetrievalHealth, RetrievalRequirements, SearchCascade, SearchTierDecision,
    SearchTierDecisionSource, SearchTierReport,
};
use crate::SearchResults;

mod query_binding;
mod receipt_binding;
mod result_binding;
mod wire;

pub use query_binding::SearchQueryBindingV1;
pub use receipt_binding::SearchCascadeReceiptBindingV2;
pub use result_binding::SearchResultsBindingV2;

/// Stable schema identifier for [`SearchCascadeReceiptV2`].
pub const SEARCH_CASCADE_RECEIPT_V2_SCHEMA: &str = "a3s/search-cascade-receipt/v2";

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

/// Version-two audit record for one caller-defined lazy retrieval cascade.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SearchCascadeReceiptV2 {
    /// Exact receipt schema identifier.
    pub schema: String,
    /// Query and complete typed query identity.
    pub query: SearchQueryBindingV1,
    /// Caller-selected structural fallback requirements.
    pub retrieval_requirements: RetrievalRequirements,
    /// Structural and operational health of the final merged result set.
    pub final_health: RetrievalHealth,
    /// Deterministic identity of every caller-visible final result field.
    pub result_set: SearchResultsBindingV2,
    /// Ordered opaque identifiers for every available tier.
    pub configured_tiers: Vec<String>,
    /// Ordered reports for tiers the caller records as executed.
    pub executed_tiers: Vec<SearchTierReport>,
    /// Whether final health satisfies `retrieval_requirements`.
    pub retrieval_requirements_met: bool,
    /// Whether every configured tier ran while the final decision still asks
    /// for another tier.
    pub exhausted: bool,
    /// Counts derived from the returned result container.
    pub counts: SearchCascadeCounts,
}

impl SearchCascadeReceiptV2 {
    /// Validates the receipt against its returned results.
    ///
    /// This proves structural self-consistency only. In particular, an
    /// external-policy decision is recorded but never semantically re-evaluated
    /// by A3S Search.
    pub fn validate(&self, results: &SearchResults) -> Result<(), SearchCascadeReceiptError> {
        self.validate_internal()?;
        validate_result_counts(&self.counts, results)?;
        self.result_set.validate(results)?;
        if self.executed_tiers.is_empty() && !is_initial_empty_results(results) {
            return Err(SearchCascadeReceiptError::OutputWithoutExecutedTier);
        }

        let recomputed = RetrievalHealth::observe(results);
        if self.final_health != recomputed {
            return Err(SearchCascadeReceiptError::FinalHealthMismatch);
        }
        if self.final_health.usable_result_count + self.final_health.invalid_result_count
            != results.items().len()
        {
            return Err(SearchCascadeReceiptError::InvalidHealthState {
                field: "final_health.result_counts".to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn validate_internal(&self) -> Result<(), SearchCascadeReceiptError> {
        if self.schema != SEARCH_CASCADE_RECEIPT_V2_SCHEMA {
            return Err(SearchCascadeReceiptError::UnsupportedSchema {
                actual: self.schema.clone(),
            });
        }
        self.query.validate()?;
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
            validate_health(
                &report.combined_health,
                &format!("executed_tiers[{index}].combined_health"),
            )?;
            if report.decision_source == SearchTierDecisionSource::RetrievalRequirements {
                let expected = if self.retrieval_requirements.is_met(&report.combined_health) {
                    SearchTierDecision::Stop
                } else {
                    SearchTierDecision::Continue
                };
                if report.decision != expected {
                    return Err(SearchCascadeReceiptError::InvalidTierDecision { index });
                }
            }
            if report.decision == SearchTierDecision::Stop && index + 1 != self.executed_tiers.len()
            {
                return Err(SearchCascadeReceiptError::TierExecutedAfterStop { index: index + 1 });
            }
        }

        if !is_canonical_sha256(&self.result_set.sha256) {
            return Err(SearchCascadeReceiptError::InvalidResultDigest);
        }

        validate_health(&self.final_health, "final_health")?;
        if let Some(last) = self.executed_tiers.last() {
            if last.combined_health != self.final_health {
                return Err(SearchCascadeReceiptError::FinalTierHealthMismatch);
            }
        }

        let requirements_met = self.retrieval_requirements.is_met(&self.final_health);
        if self.retrieval_requirements_met != requirements_met {
            return Err(SearchCascadeReceiptError::RequirementsStateMismatch);
        }
        let needs_more = self
            .executed_tiers
            .last()
            .map_or(!requirements_met, |report| {
                report.decision == SearchTierDecision::Continue
            });
        let exhausted = needs_more && self.executed_tiers.len() == self.configured_tiers.len();
        if self.exhausted != exhausted {
            return Err(SearchCascadeReceiptError::ExhaustionStateMismatch);
        }

        Ok(())
    }
}

impl Serialize for SearchCascadeReceiptV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        wire::serialize_receipt(self, serializer)
    }
}

impl<'de> Deserialize<'de> for SearchCascadeReceiptV2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        wire::deserialize_receipt(deserializer)
    }
}

/// Final merged results paired with their version-two cascade receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SearchCascadeOutcomeV2 {
    /// Internally self-consistent cascade receipt.
    pub receipt: SearchCascadeReceiptV2,
    /// Canonically merged search output.
    pub results: SearchResults,
}

impl SearchCascadeOutcomeV2 {
    /// Validates the receipt and returned results as one outcome.
    pub fn validate(&self) -> Result<(), SearchCascadeReceiptError> {
        self.receipt.validate(&self.results)
    }

    /// Returns the canonical identity of the complete validated receipt.
    pub fn receipt_binding(
        &self,
    ) -> Result<SearchCascadeReceiptBindingV2, SearchCascadeReceiptError> {
        self.validate()?;
        SearchCascadeReceiptBindingV2::new(&self.receipt)
    }
}

/// Validation failure for a versioned search cascade receipt.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SearchCascadeReceiptError {
    /// Receipt schema is unsupported.
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
    /// A structural health record contains impossible counts.
    #[error("search cascade receipt has an invalid retrieval health state at {field}")]
    InvalidHealthState { field: String },
    /// Configured tier identifiers are empty or repeated.
    #[error("search cascade receipt has an invalid tier plan at index {index}: {reason}")]
    InvalidTierPlan { index: usize, reason: &'static str },
    /// Executed tiers are not the exact ordered prefix of configured tiers.
    #[error("search cascade executed tier does not match its plan at index {index}")]
    TierPlanMismatch { index: usize },
    /// A structural tier decision disagrees with its recorded health.
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
    /// Final retrieval health cannot be recomputed from the returned output.
    #[error("search cascade final retrieval health does not match returned results")]
    FinalHealthMismatch,
    /// Last tier health differs from final health.
    #[error("search cascade final tier health does not match final health")]
    FinalTierHealthMismatch,
    /// `retrieval_requirements_met` disagrees with structural evaluation.
    #[error("search cascade retrieval-requirements state is inconsistent")]
    RequirementsStateMismatch,
    /// `exhausted` disagrees with the tier plan and final decision.
    #[error("search cascade exhaustion state is inconsistent")]
    ExhaustionStateMismatch,
}

impl SearchCascade {
    /// Consumes the cascade and returns results with a validated V2 receipt.
    pub fn finish_with_tier_plan<I, S>(
        self,
        configured_tiers: I,
    ) -> Result<SearchCascadeOutcomeV2, SearchCascadeReceiptError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let configured_tiers = configured_tiers
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let final_health = self.health();
        let retrieval_requirements_met = self.requirements.is_met(&final_health);
        let needs_more = self.needs_next_tier();
        let exhausted = needs_more && self.reports.len() == configured_tiers.len();
        let counts = counts_for_results(&self.results);
        let result_set = SearchResultsBindingV2::new(&self.results)?;
        let receipt = SearchCascadeReceiptV2 {
            schema: SEARCH_CASCADE_RECEIPT_V2_SCHEMA.to_string(),
            query: SearchQueryBindingV1::new(self.query),
            retrieval_requirements: self.requirements,
            final_health,
            result_set,
            configured_tiers,
            executed_tiers: self.reports,
            retrieval_requirements_met,
            exhausted,
            counts,
        };
        let outcome = SearchCascadeOutcomeV2 {
            receipt,
            results: self.results,
        };
        outcome.validate()?;
        Ok(outcome)
    }
}

fn validate_health(health: &RetrievalHealth, path: &str) -> Result<(), SearchCascadeReceiptError> {
    let outcome_total = health
        .successful_engine_count
        .saturating_add(health.empty_engine_count)
        .saturating_add(health.failed_engine_count)
        .saturating_add(health.timed_out_engine_count)
        .saturating_add(health.rejected_engine_count)
        .saturating_add(health.circuit_open_engine_count);
    let usable_counts_are_possible = health.unique_host_count <= health.usable_result_count
        && health.consensus_result_count <= health.usable_result_count;
    let empty_state_is_consistent = health.usable_result_count != 0
        || (health.unique_host_count == 0
            && health.contributing_engine_count == 0
            && health.consensus_result_count == 0);
    if outcome_total != health.attempted_engine_count
        || !usable_counts_are_possible
        || !empty_state_is_consistent
    {
        return Err(SearchCascadeReceiptError::InvalidHealthState {
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
