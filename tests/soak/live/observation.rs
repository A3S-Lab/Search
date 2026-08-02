use std::collections::{HashMap, HashSet};

use a3s_search::{
    EngineOutcome, EngineOutcomeKind, RetrievalRequirements, SearchCascade, SearchQuery,
};

use super::corpus::{LiveCanaryQuery, ProviderPolicy, TierCapability};
use super::driver::{AttemptReceipt, FailureStage, ProcessTreeResourceSample, UpstreamCallReceipt};
use super::rate::is_rate_limited;

#[derive(Debug)]
pub(super) struct AttemptObservation {
    pub nonempty: bool,
    pub structurally_sufficient: bool,
    pub second_tier_escalated: bool,
    pub final_tier_escalated: bool,
    pub engine_slots: u64,
    pub upstream_calls: u64,
    pub retry_attempts: u64,
    pub rate_limited_outcomes: u64,
    pub circuit_open: u64,
    pub terminal_error_kind: Option<String>,
    pub terminal_failure_stage: Option<FailureStage>,
    pub calls: Vec<UpstreamCallReceipt>,
    pub resource_samples: Vec<ProcessTreeResourceSample>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_attempt(
    expected_attempt_id: u64,
    observed_duration_ms: u64,
    query: &LiveCanaryQuery,
    capabilities: &[TierCapability],
    profiles: &[String],
    provider_policies: &[Vec<ProviderPolicy>],
    evaluated_commit: &str,
    candidate_identity: &str,
    receipt: AttemptReceipt,
) -> Result<AttemptObservation, String> {
    validate_attempt_identity(
        expected_attempt_id,
        observed_duration_ms,
        query,
        capabilities,
        profiles,
        provider_policies,
        evaluated_commit,
        candidate_identity,
        &receipt,
    )?;
    let terminal = validate_terminal(&receipt, capabilities)?;
    if terminal == Some(FailureStage::PreExecution) {
        return Ok(AttemptObservation {
            nonempty: false,
            structurally_sufficient: false,
            second_tier_escalated: false,
            final_tier_escalated: false,
            engine_slots: 0,
            upstream_calls: 0,
            retry_attempts: 0,
            rate_limited_outcomes: 0,
            circuit_open: 0,
            terminal_error_kind: receipt.terminal_error_kind,
            terminal_failure_stage: terminal,
            calls: Vec::new(),
            resource_samples: receipt.resource_samples,
        });
    }

    let tier_count = receipt.tiers.len();
    let mut cascade = SearchCascade::new(
        canary_search_query(query),
        RetrievalRequirements::for_limit(5),
    );
    let mut outcomes = Vec::new();
    let mut calls = Vec::new();
    let mut shortcuts = HashSet::new();
    let mut circuit_open = 0_u64;
    let mut second_tier_escalated = false;
    let mut final_tier_escalated = false;

    for (index, tier) in receipt.tiers.into_iter().enumerate() {
        if tier.capability != capabilities[index] {
            return Err("attempt tier does not match the sealed capability order".to_string());
        }
        if tier.profile_sha256 != profiles[index] {
            return Err("attempt tier does not match the sealed deployment profile".to_string());
        }
        if index > 0 && !cascade.needs_next_tier() {
            return Err(
                "driver eagerly executed a tier after retrieval requirements were satisfied"
                    .to_string(),
            );
        }
        match index {
            1 => second_tier_escalated = true,
            2 => final_tier_escalated = true,
            _ => {}
        }
        let tier_outcomes = tier.results.outcomes();
        if tier_outcomes.is_empty() {
            return Err("executed tier omitted per-engine outcomes".to_string());
        }
        for outcome in tier_outcomes {
            if !shortcuts.insert((index, outcome.shortcut.clone())) {
                return Err("executed tier repeated an engine outcome".to_string());
            }
            circuit_open = circuit_open
                .saturating_add(u64::from(outcome.kind == EngineOutcomeKind::CircuitOpen));
            outcomes.push(outcome.clone());
        }
        validate_result_attribution(tier_outcomes, tier.results.items())?;
        validate_calls(
            tier_outcomes,
            &tier.calls,
            &provider_policies[index],
            receipt.attempt_duration_ms,
        )?;
        calls.extend(tier.calls);
        cascade.push_tier(format!("{:?}", tier.capability), tier.results);
    }

    if calls
        .windows(2)
        .any(|pair| pair[1].started_offset_ms < pair[0].started_offset_ms)
    {
        return Err("call receipts are not globally time ordered".to_string());
    }
    if terminal.is_none() && cascade.needs_next_tier() && tier_count < capabilities.len() {
        return Err("driver stopped before an available fallback tier".to_string());
    }
    let retry_attempts = calls.iter().filter(|call| call.is_retry).count() as u64;
    let upstream_calls = calls.len() as u64 - retry_attempts;
    let rate_limited_outcomes = calls
        .iter()
        .filter(|call| is_rate_limited(call.failure_kind.as_deref()))
        .count() as u64;
    Ok(AttemptObservation {
        nonempty: !cascade.results().items().is_empty(),
        structurally_sufficient: !cascade.needs_next_tier(),
        second_tier_escalated,
        final_tier_escalated,
        engine_slots: outcomes.len() as u64,
        upstream_calls,
        retry_attempts,
        rate_limited_outcomes,
        circuit_open,
        terminal_error_kind: receipt.terminal_error_kind,
        terminal_failure_stage: terminal,
        calls,
        resource_samples: receipt.resource_samples,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_attempt_identity(
    expected_attempt_id: u64,
    observed_duration_ms: u64,
    query: &LiveCanaryQuery,
    capabilities: &[TierCapability],
    profiles: &[String],
    provider_policies: &[Vec<ProviderPolicy>],
    evaluated_commit: &str,
    candidate_identity: &str,
    receipt: &AttemptReceipt,
) -> Result<(), String> {
    if receipt.message_type != "attempt"
        || receipt.schema_version != 3
        || receipt.attempt_id != expected_attempt_id
        || receipt.query_id != query.id
        || receipt.evaluated_commit != evaluated_commit
        || receipt.candidate_sha256 != candidate_identity
    {
        return Err("attempt identity does not match the sealed request".to_string());
    }
    if receipt.attempt_duration_ms > observed_duration_ms {
        return Err("driver duration exceeds the observed request duration".to_string());
    }
    if receipt.tiers.len() > capabilities.len()
        || capabilities.len() != profiles.len()
        || capabilities.len() != provider_policies.len()
    {
        return Err("attempt returned more tiers than the sealed manifest".to_string());
    }
    Ok(())
}

fn validate_terminal(
    receipt: &AttemptReceipt,
    capabilities: &[TierCapability],
) -> Result<Option<FailureStage>, String> {
    let terminal = match (
        receipt.terminal_error_kind.as_deref(),
        receipt.terminal_failure_stage,
    ) {
        (None, None) => None,
        (Some(kind), Some(stage)) => {
            validate_failure_kind(kind, "terminal error")?;
            Some(stage)
        }
        _ => {
            return Err(
                "terminal error kind and failure stage must be present together".to_string(),
            )
        }
    };
    match terminal {
        Some(FailureStage::PreExecution) if !receipt.tiers.is_empty() => {
            Err("pre-execution failure cannot claim executed tiers".to_string())
        }
        Some(FailureStage::PreExecution) => Ok(terminal),
        Some(stage) if receipt.tiers.last().map(|tier| tier.capability) != stage.capability() => {
            Err("terminal failure stage must retain its executed tier facts".to_string())
        }
        Some(_) if receipt.tiers.len() != capabilities.len() => {
            Err("terminal attempt stopped before an available fallback tier".to_string())
        }
        Some(_) => Ok(terminal),
        None if receipt.tiers.is_empty() => {
            Err("completed attempt did not execute the first sealed tier".to_string())
        }
        None => Ok(None),
    }
}

fn validate_calls(
    outcomes: &[EngineOutcome],
    calls: &[UpstreamCallReceipt],
    allowed_provider_policies: &[ProviderPolicy],
    attempt_duration_ms: u64,
) -> Result<(), String> {
    let outcome_shortcuts = outcomes
        .iter()
        .map(|outcome| (outcome.shortcut.as_str(), outcome.kind))
        .collect::<HashMap<_, _>>();
    let allowed_scopes = allowed_provider_policies
        .iter()
        .map(|policy| policy.scope.as_str())
        .collect::<HashSet<_>>();
    let mut by_engine = HashMap::<&str, Vec<&UpstreamCallReceipt>>::new();
    let mut previous_start = 0_u64;
    for call in calls {
        validate_provider_scope(&call.provider_scope)?;
        if !allowed_scopes.contains(call.provider_scope.as_str()) {
            return Err("call receipt rotated outside the sealed provider scopes".to_string());
        }
        if call.engine_shortcut.is_empty() || call.engine_shortcut.len() > 128 {
            return Err("call receipt has an invalid engine shortcut".to_string());
        }
        if call.started_offset_ms < previous_start
            || call.ended_offset_ms < call.started_offset_ms
            || call.ended_offset_ms > attempt_duration_ms
        {
            return Err("call receipt has invalid monotonic offsets".to_string());
        }
        previous_start = call.started_offset_ms;
        let Some(kind) = outcome_shortcuts.get(call.engine_shortcut.as_str()) else {
            return Err("call receipt has no matching engine outcome".to_string());
        };
        if !is_upstream_call(*kind) {
            return Err("circuit-open or rejected outcome claimed an upstream call".to_string());
        }
        validate_call_failure(call)?;
        by_engine
            .entry(call.engine_shortcut.as_str())
            .or_default()
            .push(call);
    }
    for outcome in outcomes {
        let engine_calls = by_engine
            .get(outcome.shortcut.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        if is_upstream_call(outcome.kind) {
            validate_engine_call_chain(engine_calls)?;
        } else if !engine_calls.is_empty() {
            return Err("non-upstream outcome retained call receipts".to_string());
        }
    }
    Ok(())
}

fn validate_call_failure(call: &UpstreamCallReceipt) -> Result<(), String> {
    if let Some(kind) = call.failure_kind.as_deref() {
        validate_failure_kind(kind, "call failure")?;
    }
    if call.retryable && call.failure_kind.is_none() {
        return Err("successful call cannot authorize a retry".to_string());
    }
    if call.retry_after_seconds.is_some() && (!call.retryable || call.failure_kind.is_none()) {
        return Err("Retry-After requires a retryable failed call".to_string());
    }
    if call
        .retry_after_seconds
        .is_some_and(|seconds| seconds > 86_400)
    {
        return Err("call Retry-After exceeds the bounded maximum".to_string());
    }
    Ok(())
}

fn validate_engine_call_chain(calls: &[&UpstreamCallReceipt]) -> Result<(), String> {
    let Some(initial) = calls.first() else {
        return Err("each upstream outcome must bind one initial call".to_string());
    };
    if initial.is_retry || calls.iter().skip(1).any(|call| !call.is_retry) {
        return Err("each engine must start with exactly one initial call".to_string());
    }
    for pair in calls.windows(2) {
        let previous = pair[0];
        let retry = pair[1];
        if retry.provider_scope != initial.provider_scope {
            return Err("retry rotated outside its initial provider scope".to_string());
        }
        if retry.started_offset_ms < previous.ended_offset_ms {
            return Err("one engine's retry calls must be serial".to_string());
        }
        if previous.failure_kind.is_none() || !previous.retryable {
            return Err("retry was not authorized by the preceding failed call".to_string());
        }
    }
    Ok(())
}

fn validate_result_attribution(
    outcomes: &[EngineOutcome],
    results: &[a3s_search::SearchResult],
) -> Result<(), String> {
    let successful = outcomes
        .iter()
        .filter(|outcome| outcome.kind == EngineOutcomeKind::Success)
        .map(|outcome| outcome.engine.as_str())
        .collect::<HashSet<_>>();
    for result in results {
        if result.engines.is_empty()
            || result
                .engines
                .iter()
                .any(|engine| !successful.contains(engine.as_str()))
        {
            return Err("result provenance is not backed by a successful outcome".to_string());
        }
    }
    for outcome in outcomes {
        let attributed = results
            .iter()
            .filter(|result| result.engines.contains(&outcome.engine))
            .count();
        if attributed > outcome.result_count {
            return Err("result provenance exceeds the engine's raw result count".to_string());
        }
    }
    Ok(())
}

fn validate_provider_scope(scope: &str) -> Result<(), String> {
    let digest = scope.strip_prefix("sha256:").unwrap_or_default();
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("provider scope must be an opaque lowercase SHA-256 identity".to_string());
    }
    Ok(())
}

fn validate_failure_kind(kind: &str, description: &str) -> Result<(), String> {
    if kind.is_empty()
        || kind.len() > 64
        || !kind.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        return Err(format!(
            "{description} kind is not bounded low-cardinality text"
        ));
    }
    Ok(())
}

fn canary_search_query(query: &LiveCanaryQuery) -> SearchQuery {
    let query_builder = SearchQuery::new(query.query.clone());
    match query.language.as_deref() {
        Some(language) => query_builder.with_language(language),
        None => query_builder,
    }
}

fn is_upstream_call(kind: EngineOutcomeKind) -> bool {
    matches!(
        kind,
        EngineOutcomeKind::Success
            | EngineOutcomeKind::Empty
            | EngineOutcomeKind::Failure
            | EngineOutcomeKind::Timeout
    )
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
