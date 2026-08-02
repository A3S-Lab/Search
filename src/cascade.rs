//! Structural retrieval health and caller-controlled tier cascades.

use std::collections::HashSet;
use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::{EngineOutcomeKind, SearchQuery, SearchResult, SearchResults};

mod receipt;

pub use receipt::{
    SearchCascadeCounts, SearchCascadeOutcomeV2, SearchCascadeReceiptBindingV2,
    SearchCascadeReceiptError, SearchCascadeReceiptV2, SearchQueryBindingV1,
    SearchResultsBindingV2, SEARCH_CASCADE_RECEIPT_V2_SCHEMA,
};

/// Observable, non-semantic health signals for an aggregated retrieval result.
///
/// These fields describe transport outcomes, result structure, and source
/// provenance. They do not estimate relevance, correctness, evidence coverage,
/// or whether a result answers the query.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct RetrievalHealth {
    /// Results with an absolute HTTP(S) URL and a host.
    pub usable_result_count: usize,
    /// Results without an absolute HTTP(S) URL and host.
    pub invalid_result_count: usize,
    /// Distinct normalized hosts represented by usable results.
    pub unique_host_count: usize,
    /// Distinct logical sources that contributed at least one usable result.
    ///
    /// The field retains its original name for wire compatibility. Engines
    /// exposing multiple transports must use one shared [`crate::EngineConfig::name`].
    pub contributing_engine_count: usize,
    /// Usable results independently returned by at least two logical sources.
    pub consensus_result_count: usize,
    /// Engine attempts with a typed terminal outcome.
    pub attempted_engine_count: usize,
    /// Engine attempts that returned non-empty structured output.
    pub successful_engine_count: usize,
    /// Engine attempts that completed with empty structured output.
    pub empty_engine_count: usize,
    /// Engine attempts that returned a typed failure.
    pub failed_engine_count: usize,
    /// Engine attempts that exceeded their timeout.
    pub timed_out_engine_count: usize,
    /// Engine attempts rejected by local concurrency isolation.
    pub rejected_engine_count: usize,
    /// Engines skipped because a circuit or local health gate was open.
    pub circuit_open_engine_count: usize,
}

impl RetrievalHealth {
    /// Observes structural and operational health without inspecting result text.
    pub fn observe(results: &SearchResults) -> Self {
        let mut health = Self::observe_items(results.items());
        health.attempted_engine_count = results.outcomes().len();
        for outcome in results.outcomes() {
            let counter = match outcome.kind {
                EngineOutcomeKind::Success => &mut health.successful_engine_count,
                EngineOutcomeKind::Empty => &mut health.empty_engine_count,
                EngineOutcomeKind::Failure => &mut health.failed_engine_count,
                EngineOutcomeKind::Timeout => &mut health.timed_out_engine_count,
                EngineOutcomeKind::Rejected => &mut health.rejected_engine_count,
                EngineOutcomeKind::CircuitOpen => &mut health.circuit_open_engine_count,
            };
            *counter = counter.saturating_add(1);
        }
        health
    }

    /// Observes only the supplied caller-visible result rows.
    ///
    /// Operational outcome counters remain zero because the iterator does not
    /// carry the complete engine-attempt record.
    pub fn observe_items<'a>(items: impl IntoIterator<Item = &'a SearchResult>) -> Self {
        let mut health = Self::default();
        let mut hosts = HashSet::new();
        let mut engines = HashSet::new();

        for result in items {
            let usable_host = normalized_usable_host(result);
            let Some(host) = usable_host else {
                health.invalid_result_count = health.invalid_result_count.saturating_add(1);
                continue;
            };

            health.usable_result_count = health.usable_result_count.saturating_add(1);
            hosts.insert(host);
            engines.extend(result.engines.iter().cloned());
            if result.engines.len() >= 2 {
                health.consensus_result_count = health.consensus_result_count.saturating_add(1);
            }
        }

        health.unique_host_count = hosts.len();
        health.contributing_engine_count = engines.len();
        health
    }
}

pub(crate) fn normalized_usable_host(result: &SearchResult) -> Option<String> {
    url::Url::parse(result.url.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_ascii_lowercase())
        })
}

/// Caller-selected structural requirements for operational fallback.
///
/// The requirements deliberately contain no text, topic, language, publisher,
/// domain, or semantic thresholds. Applications that need semantic evaluation
/// must make that decision outside A3S Search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct RetrievalRequirements {
    /// Minimum usable HTTP(S) results.
    pub min_usable_results: usize,
    /// Minimum distinct normalized hosts.
    pub min_unique_hosts: usize,
    /// Minimum distinct contributing logical sources.
    pub min_contributing_engines: usize,
    /// Minimum results independently returned by at least two logical sources.
    pub min_consensus_results: usize,
}

impl RetrievalRequirements {
    /// Creates structural requirements for a requested display limit.
    pub fn for_limit(limit: usize) -> Self {
        let target = limit.min(5);
        Self {
            min_usable_results: target,
            min_unique_hosts: target.min(3),
            min_contributing_engines: target.min(2),
            min_consensus_results: 0,
        }
    }

    /// Observes the supplied result container.
    pub fn evaluate(&self, results: &SearchResults) -> RetrievalHealth {
        RetrievalHealth::observe(results)
    }

    /// Observes a caller-visible selection without inspecting result text.
    pub fn evaluate_items<'a>(
        &self,
        items: impl IntoIterator<Item = &'a SearchResult>,
    ) -> RetrievalHealth {
        RetrievalHealth::observe_items(items)
    }

    /// Returns whether the structural result requirements are met.
    pub fn is_met(&self, health: &RetrievalHealth) -> bool {
        health.usable_result_count >= self.min_usable_results
            && health.unique_host_count >= self.min_unique_hosts
            && health.contributing_engine_count >= self.min_contributing_engines
            && health.consensus_result_count >= self.min_consensus_results
    }
}

impl Default for RetrievalRequirements {
    fn default() -> Self {
        Self::for_limit(10)
    }
}

/// Decision after one tier has been merged into a search cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchTierDecision {
    /// Stop before constructing another retrieval tier.
    Stop,
    /// Run a lower tier if one is available.
    Continue,
}

/// Authority that made a tier-continuation decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchTierDecisionSource {
    /// A3S Search applied only the recorded structural requirements.
    RetrievalRequirements,
    /// An external caller or evaluator supplied the decision.
    ExternalPolicy,
}

/// Audit record for one executed search tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SearchTierReport {
    /// Caller-defined tier identifier.
    pub tier: String,
    /// Structural and operational health after this tier was merged.
    pub combined_health: RetrievalHealth,
    /// Cascade decision after this tier.
    pub decision: SearchTierDecision,
    /// Authority that made the decision.
    pub decision_source: SearchTierDecisionSource,
}

/// Stateful merger for lazily executed retrieval tiers.
///
/// The default path uses only structural requirements. A semantic evaluator can
/// remain outside this crate and submit its opaque decision through
/// [`SearchCascade::push_tier_with_decision`].
#[derive(Debug)]
pub struct SearchCascade {
    pub(crate) query: SearchQuery,
    pub(crate) requirements: RetrievalRequirements,
    pub(crate) results: SearchResults,
    pub(crate) reports: Vec<SearchTierReport>,
}

impl SearchCascade {
    /// Starts a cascade for one query and structural retrieval requirements.
    pub fn new(query: SearchQuery, requirements: RetrievalRequirements) -> Self {
        Self {
            query,
            requirements,
            results: SearchResults::new(),
            reports: Vec::new(),
        }
    }

    /// Merges one tier and applies only the structural requirements.
    pub fn push_tier(
        &mut self,
        tier: impl Into<String>,
        results: SearchResults,
    ) -> SearchTierDecision {
        self.results.merge(results);
        let health = self.health();
        let decision = if self.requirements.is_met(&health) {
            SearchTierDecision::Stop
        } else {
            SearchTierDecision::Continue
        };
        self.record_report(
            tier,
            health,
            decision,
            SearchTierDecisionSource::RetrievalRequirements,
        );
        decision
    }

    /// Merges one tier and records a decision made by an external policy.
    ///
    /// A3S Search does not inspect, reproduce, or validate the policy's semantic
    /// reasoning. The receipt records that the decision came from outside.
    pub fn push_tier_with_decision(
        &mut self,
        tier: impl Into<String>,
        results: SearchResults,
        decision: SearchTierDecision,
    ) -> SearchTierDecision {
        self.results.merge(results);
        let health = self.health();
        self.record_report(
            tier,
            health,
            decision,
            SearchTierDecisionSource::ExternalPolicy,
        );
        decision
    }

    /// Executes one tier only while the recorded cascade decision requires it.
    pub async fn run_tier_if_needed<F, Fut>(
        &mut self,
        tier: impl Into<String>,
        run: F,
    ) -> Option<SearchTierDecision>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = SearchResults>,
    {
        if !self.needs_next_tier() {
            return None;
        }
        Some(self.push_tier(tier, run().await))
    }

    /// Returns structural and operational health for all merged tiers.
    pub fn health(&self) -> RetrievalHealth {
        self.requirements.evaluate(&self.results)
    }

    /// Returns the structural requirements used by the default decision path.
    pub const fn requirements(&self) -> RetrievalRequirements {
        self.requirements
    }

    /// Returns whether the current recorded decision requires another tier.
    pub fn needs_next_tier(&self) -> bool {
        self.reports.last().map_or_else(
            || !self.requirements.is_met(&self.health()),
            |report| report.decision == SearchTierDecision::Continue,
        )
    }

    /// Returns the current combined results.
    pub fn results(&self) -> &SearchResults {
        &self.results
    }

    /// Returns the executed-tier audit trail.
    pub fn reports(&self) -> &[SearchTierReport] {
        &self.reports
    }

    /// Consumes the cascade and returns its combined results.
    pub fn into_results(self) -> SearchResults {
        self.results
    }

    fn record_report(
        &mut self,
        tier: impl Into<String>,
        combined_health: RetrievalHealth,
        decision: SearchTierDecision,
        decision_source: SearchTierDecisionSource,
    ) {
        self.reports.push(SearchTierReport {
            tier: tier.into(),
            combined_health,
            decision,
            decision_source,
        });
    }
}

#[cfg(test)]
#[path = "cascade/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cascade/receipt_tests.rs"]
mod receipt_tests;
