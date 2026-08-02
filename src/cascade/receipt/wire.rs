//! Frozen JSON wire representation for version-two cascade receipts.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    SearchCascadeCounts, SearchCascadeReceiptV2, SearchQueryBindingV1, SearchResultsBindingV2,
};
use crate::{RetrievalHealth, RetrievalRequirements, SearchTierReport};

pub(super) fn serialize_receipt<S>(
    value: &SearchCascadeReceiptV2,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    SearchCascadeReceiptWireV2 {
        schema: value.schema.clone(),
        query: value.query.clone(),
        retrieval_requirements: value.retrieval_requirements,
        final_health: value.final_health,
        result_set: value.result_set.clone(),
        configured_tiers: value.configured_tiers.clone(),
        executed_tiers: value.executed_tiers.clone(),
        retrieval_requirements_met: value.retrieval_requirements_met,
        exhausted: value.exhausted,
        counts: value.counts,
    }
    .serialize(serializer)
}

pub(super) fn deserialize_receipt<'de, D>(
    deserializer: D,
) -> Result<SearchCascadeReceiptV2, D::Error>
where
    D: Deserializer<'de>,
{
    let wire = SearchCascadeReceiptWireV2::deserialize(deserializer)?;
    Ok(SearchCascadeReceiptV2 {
        schema: wire.schema,
        query: wire.query,
        retrieval_requirements: wire.retrieval_requirements,
        final_health: wire.final_health,
        result_set: wire.result_set,
        configured_tiers: wire.configured_tiers,
        executed_tiers: wire.executed_tiers,
        retrieval_requirements_met: wire.retrieval_requirements_met,
        exhausted: wire.exhausted,
        counts: wire.counts,
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchCascadeReceiptWireV2 {
    schema: String,
    query: SearchQueryBindingV1,
    retrieval_requirements: RetrievalRequirements,
    final_health: RetrievalHealth,
    result_set: SearchResultsBindingV2,
    configured_tiers: Vec<String>,
    executed_tiers: Vec<SearchTierReport>,
    retrieval_requirements_met: bool,
    exhausted: bool,
    counts: SearchCascadeCounts,
}
