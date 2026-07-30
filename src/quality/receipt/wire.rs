//! Frozen JSON wire representation for version-one cascade receipts.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    SearchCascadeCounts, SearchCascadeReceiptV1, SearchQuality, SearchQualityFloor,
    SearchQueryBindingV1, SearchResultsBindingV1, SearchTierDecision, SearchTierReport,
};

pub(super) fn serialize_receipt<S>(
    value: &SearchCascadeReceiptV1,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    SearchCascadeReceiptWireV1 {
        schema: value.schema.clone(),
        query: value.query.clone(),
        quality_floor: value.quality_floor.into(),
        final_quality: value.final_quality.into(),
        result_set: value.result_set.clone(),
        configured_tiers: value.configured_tiers.clone(),
        executed_tiers: value
            .executed_tiers
            .iter()
            .map(SearchTierReportWireV1::from_report)
            .collect(),
        quality_floor_met: value.quality_floor_met,
        exhausted_below_floor: value.exhausted_below_floor,
        counts: value.counts,
    }
    .serialize(serializer)
}

pub(super) fn deserialize_receipt<'de, D>(
    deserializer: D,
) -> Result<SearchCascadeReceiptV1, D::Error>
where
    D: Deserializer<'de>,
{
    let wire = SearchCascadeReceiptWireV1::deserialize(deserializer)?;
    Ok(SearchCascadeReceiptV1 {
        schema: wire.schema,
        query: wire.query,
        quality_floor: wire.quality_floor.into_floor(),
        final_quality: wire.final_quality.into_quality(),
        result_set: wire.result_set,
        configured_tiers: wire.configured_tiers,
        executed_tiers: wire
            .executed_tiers
            .into_iter()
            .map(SearchTierReportWireV1::into_report)
            .collect(),
        quality_floor_met: wire.quality_floor_met,
        exhausted_below_floor: wire.exhausted_below_floor,
        counts: wire.counts,
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQualityWireV1 {
    usable_result_count: usize,
    unique_host_count: usize,
    contributing_engine_count: usize,
    consensus_result_count: usize,
    aligned_result_count: usize,
    mean_query_match: f64,
}

impl From<SearchQuality> for SearchQualityWireV1 {
    fn from(value: SearchQuality) -> Self {
        let SearchQuality {
            usable_result_count,
            unique_host_count,
            contributing_engine_count,
            consensus_result_count,
            aligned_result_count,
            mean_query_match,
        } = value;
        Self {
            usable_result_count,
            unique_host_count,
            contributing_engine_count,
            consensus_result_count,
            aligned_result_count,
            mean_query_match,
        }
    }
}

impl SearchQualityWireV1 {
    fn into_quality(self) -> SearchQuality {
        SearchQuality {
            usable_result_count: self.usable_result_count,
            unique_host_count: self.unique_host_count,
            contributing_engine_count: self.contributing_engine_count,
            consensus_result_count: self.consensus_result_count,
            aligned_result_count: self.aligned_result_count,
            mean_query_match: self.mean_query_match,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQualityFloorWireV1 {
    min_usable_results: usize,
    min_unique_hosts: usize,
    min_contributing_engines: usize,
    min_aligned_results: usize,
    min_consensus_results: usize,
    min_query_match: f64,
    min_mean_query_match: f64,
}

impl From<SearchQualityFloor> for SearchQualityFloorWireV1 {
    fn from(value: SearchQualityFloor) -> Self {
        let SearchQualityFloor {
            min_usable_results,
            min_unique_hosts,
            min_contributing_engines,
            min_aligned_results,
            min_consensus_results,
            min_query_match,
            min_mean_query_match,
        } = value;
        Self {
            min_usable_results,
            min_unique_hosts,
            min_contributing_engines,
            min_aligned_results,
            min_consensus_results,
            min_query_match,
            min_mean_query_match,
        }
    }
}

impl SearchQualityFloorWireV1 {
    fn into_floor(self) -> SearchQualityFloor {
        SearchQualityFloor {
            min_usable_results: self.min_usable_results,
            min_unique_hosts: self.min_unique_hosts,
            min_contributing_engines: self.min_contributing_engines,
            min_aligned_results: self.min_aligned_results,
            min_consensus_results: self.min_consensus_results,
            min_query_match: self.min_query_match,
            min_mean_query_match: self.min_mean_query_match,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SearchTierDecisionWireV1 {
    Stop,
    Continue,
}

impl From<SearchTierDecision> for SearchTierDecisionWireV1 {
    fn from(value: SearchTierDecision) -> Self {
        match value {
            SearchTierDecision::Stop => Self::Stop,
            SearchTierDecision::Continue => Self::Continue,
        }
    }
}

impl From<SearchTierDecisionWireV1> for SearchTierDecision {
    fn from(value: SearchTierDecisionWireV1) -> Self {
        match value {
            SearchTierDecisionWireV1::Stop => Self::Stop,
            SearchTierDecisionWireV1::Continue => Self::Continue,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchTierReportWireV1 {
    tier: String,
    combined_quality: SearchQualityWireV1,
    decision: SearchTierDecisionWireV1,
}

impl SearchTierReportWireV1 {
    fn from_report(value: &SearchTierReport) -> Self {
        Self {
            tier: value.tier.clone(),
            combined_quality: value.combined_quality.into(),
            decision: value.decision.into(),
        }
    }

    fn into_report(self) -> SearchTierReport {
        SearchTierReport {
            tier: self.tier,
            combined_quality: self.combined_quality.into_quality(),
            decision: self.decision.into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchCascadeReceiptWireV1 {
    schema: String,
    query: SearchQueryBindingV1,
    quality_floor: SearchQualityFloorWireV1,
    final_quality: SearchQualityWireV1,
    result_set: SearchResultsBindingV1,
    configured_tiers: Vec<String>,
    executed_tiers: Vec<SearchTierReportWireV1>,
    quality_floor_met: bool,
    exhausted_below_floor: bool,
    counts: SearchCascadeCounts,
}
