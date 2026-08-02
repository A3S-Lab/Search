use super::*;
use a3s_search::{EngineOutcome, SearchResult, SearchResults};

use super::super::driver::TierReceipt;

fn query() -> LiveCanaryQuery {
    LiveCanaryQuery {
        id: "case-1".to_string(),
        query: "independent query".to_string(),
        language: None,
    }
}

fn capabilities() -> Vec<TierCapability> {
    vec![
        TierCapability::Headless,
        TierCapability::HttpRss,
        TierCapability::Api,
    ]
}

fn profiles() -> Vec<String> {
    ['3', '2', '1'].into_iter().map(scope).collect()
}

fn scope(digest: char) -> String {
    format!("sha256:{}", digest.to_string().repeat(64))
}

fn provider_policies() -> Vec<Vec<ProviderPolicy>> {
    ['c', 'b', 'a']
        .into_iter()
        .map(|digest| {
            vec![ProviderPolicy {
                scope: scope(digest),
                minimum_interval_seconds: 60,
            }]
        })
        .enumerate()
        .map(|(index, mut policies)| {
            if index == 0 {
                policies.push(ProviderPolicy {
                    scope: scope('e'),
                    minimum_interval_seconds: 60,
                });
            }
            policies
        })
        .collect()
}

fn tier(
    capability: TierCapability,
    shortcut: &str,
    structurally_sufficient: bool,
    offset_ms: u64,
) -> TierReceipt {
    let mut results = SearchResults::new();
    let count = if structurally_sufficient { 5 } else { 1 };
    for index in 0..count {
        results.add_result(
            SearchResult::new(
                format!("https://host-{index}.invalid/independent-query"),
                format!("independent query result {index}"),
                "independent query evidence",
            )
            .with_engine(shortcut, index as u32 + 1),
        );
    }
    let outcome: EngineOutcome = serde_json::from_value(serde_json::json!({
        "engine": shortcut,
        "shortcut": shortcut,
        "kind": "success",
        "result_count": count,
        "duration_ms": 10
    }))
    .unwrap();
    results.add_outcome(outcome);
    let digest = match capability {
        TierCapability::Api => 'a',
        TierCapability::HttpRss => 'b',
        TierCapability::Headless => 'c',
    };
    TierReceipt {
        capability,
        profile_sha256: match capability {
            TierCapability::Api => scope('1'),
            TierCapability::HttpRss => scope('2'),
            TierCapability::Headless => scope('3'),
        },
        results,
        calls: vec![UpstreamCallReceipt {
            provider_scope: scope(digest),
            engine_shortcut: shortcut.to_string(),
            started_offset_ms: offset_ms,
            ended_offset_ms: offset_ms + 10,
            is_retry: false,
            failure_kind: None,
            retryable: false,
            retry_after_seconds: None,
        }],
    }
}

fn healthy_headless_tier(offset_ms: u64) -> TierReceipt {
    let mut tier = tier(TierCapability::Headless, "headless", true, offset_ms);
    let peer = "headless-peer";
    tier.results.items_mut()[0].engines.insert(peer.to_string());
    tier.results.items_mut()[0].positions.push(1);
    tier.results.add_outcome(
        serde_json::from_value(serde_json::json!({
            "engine": peer,
            "shortcut": peer,
            "kind": "success",
            "result_count": 1,
            "duration_ms": 10
        }))
        .unwrap(),
    );
    tier.calls.push(UpstreamCallReceipt {
        provider_scope: scope('e'),
        engine_shortcut: peer.to_string(),
        started_offset_ms: offset_ms,
        ended_offset_ms: offset_ms + 10,
        is_retry: false,
        failure_kind: None,
        retryable: false,
        retry_after_seconds: None,
    });
    tier
}

fn receipt(tiers: Vec<TierReceipt>) -> AttemptReceipt {
    AttemptReceipt {
        message_type: "attempt".to_string(),
        schema_version: 3,
        attempt_id: 1,
        query_id: "case-1".to_string(),
        evaluated_commit: "a".repeat(40),
        candidate_sha256: scope('d'),
        terminal_error_kind: None,
        terminal_failure_stage: None,
        attempt_duration_ms: 100,
        resource_samples: Vec::new(),
        tiers,
    }
}

fn evaluate(receipt: AttemptReceipt) -> Result<AttemptObservation, String> {
    evaluate_attempt(
        1,
        100,
        &query(),
        &capabilities(),
        &profiles(),
        &provider_policies(),
        &"a".repeat(40),
        &scope('d'),
        receipt,
    )
}

fn mark_terminal(receipt: &mut AttemptReceipt, stage: FailureStage) {
    receipt.terminal_error_kind = Some("driver_terminal".to_string());
    receipt.terminal_failure_stage = Some(stage);
}

fn exhausted_tiers() -> Vec<TierReceipt> {
    vec![
        tier(TierCapability::Headless, "headless", false, 0),
        tier(TierCapability::HttpRss, "http", false, 20),
        tier(TierCapability::Api, "api", false, 40),
    ]
}

#[test]
fn healthy_first_tier_prevents_eager_fallback() {
    let observation = evaluate(receipt(vec![healthy_headless_tier(0)])).unwrap();
    assert!(observation.structurally_sufficient);
    assert!(!observation.second_tier_escalated);
    assert!(!observation.final_tier_escalated);

    let eager = receipt(vec![
        healthy_headless_tier(0),
        tier(TierCapability::HttpRss, "secondary", false, 20),
    ]);
    assert!(evaluate(eager).is_err());
}

#[test]
fn tier_receipt_must_bind_the_precommitted_deployment_profile() {
    let mut headless = tier(TierCapability::Headless, "headless", true, 0);
    headless.profile_sha256 = scope('9');
    assert!(evaluate(receipt(vec![headless]))
        .unwrap_err()
        .contains("sealed deployment profile"));
}

#[test]
fn insufficient_completed_attempt_cannot_skip_an_available_tier() {
    let early = receipt(vec![tier(TierCapability::Headless, "headless", false, 0)]);
    assert!(evaluate(early)
        .unwrap_err()
        .contains("stopped before an available fallback"));
}

#[test]
fn terminal_receipt_is_accepted_only_after_every_fallback_tier() {
    let mut terminal = receipt(vec![tier(TierCapability::Headless, "headless", false, 0)]);
    mark_terminal(&mut terminal, FailureStage::Headless);
    assert!(evaluate(terminal)
        .unwrap_err()
        .contains("stopped before an available fallback"));

    let mut exhausted = receipt(exhausted_tiers());
    mark_terminal(&mut exhausted, FailureStage::Api);
    let observation = evaluate(exhausted).unwrap();
    assert_eq!(observation.engine_slots, 3);
    assert_eq!(observation.upstream_calls, 3);
    assert_eq!(observation.terminal_failure_stage, Some(FailureStage::Api));

    let mut after_http = receipt(vec![
        tier(TierCapability::Headless, "headless", false, 0),
        tier(TierCapability::HttpRss, "http", false, 20),
    ]);
    mark_terminal(&mut after_http, FailureStage::HttpRss);
    assert!(evaluate(after_http)
        .unwrap_err()
        .contains("stopped before an available fallback"));

    let mut mismatched = receipt(Vec::new());
    mark_terminal(&mut mismatched, FailureStage::Headless);
    assert!(evaluate(mismatched).is_err());
}

#[test]
fn explicit_pre_execution_failure_is_distinct_and_fact_free() {
    let mut terminal = receipt(Vec::new());
    mark_terminal(&mut terminal, FailureStage::PreExecution);
    let observation = evaluate(terminal).unwrap();
    assert_eq!(
        observation.terminal_failure_stage,
        Some(FailureStage::PreExecution)
    );
    assert_eq!(observation.upstream_calls, 0);
}

#[test]
fn deduplication_or_limit_may_reduce_attribution_below_raw_count() {
    let mut headless = tier(TierCapability::Headless, "headless", false, 0);
    let value = serde_json::to_value(&headless.results).unwrap();
    let mut value = value;
    value["outcomes"][0]["result_count"] = serde_json::json!(2);
    headless.results = serde_json::from_value(value).unwrap();
    let mut tiers = exhausted_tiers();
    tiers[0] = headless;
    assert!(evaluate(receipt(tiers)).is_ok());
}

#[test]
fn retry_must_follow_a_serial_retryable_failure_on_the_same_scope() {
    let mut headless = tier(TierCapability::Headless, "headless", false, 0);
    headless.calls[0].failure_kind = Some("temporary".to_string());
    headless.calls[0].retryable = true;
    headless.calls.push(UpstreamCallReceipt {
        provider_scope: scope('c'),
        engine_shortcut: "headless".to_string(),
        started_offset_ms: 20,
        ended_offset_ms: 30,
        is_retry: true,
        failure_kind: None,
        retryable: false,
        retry_after_seconds: None,
    });
    let mut tiers = exhausted_tiers();
    tiers[0] = headless;
    assert!(evaluate(receipt(tiers)).is_ok());

    let mut rotated = tier(TierCapability::Headless, "headless", false, 0);
    rotated.calls[0].failure_kind = Some("temporary".to_string());
    rotated.calls[0].retryable = true;
    let mut retry = rotated.calls[0].clone();
    retry.is_retry = true;
    retry.provider_scope = scope('e');
    retry.started_offset_ms = 20;
    retry.ended_offset_ms = 30;
    rotated.calls.push(retry);
    let mut tiers = exhausted_tiers();
    tiers[0] = rotated;
    assert!(evaluate(receipt(tiers)).is_err());
}
