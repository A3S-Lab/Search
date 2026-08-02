use super::*;

fn transient_failure() -> EngineFailure {
    EngineFailure::new("engine", "provider_transport", "offline").with_transient(true)
}

#[test]
fn terminal_failure_opens_immediately_and_is_shared() {
    let breaker = CircuitBreaker::default();
    let shared = breaker.clone();
    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&EngineFailure::new(
            "API",
            "provider_quota",
            "quota exhausted",
        ));

    let open = shared.acquire("api").unwrap_err();
    assert!(open.retry_after > Duration::ZERO);
    assert_eq!(shared.snapshot("api").state, CircuitState::Open);
}

#[test]
fn interactive_challenge_opens_immediately_without_provider_rules() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 10,
        open_jitter_ratio: 0.0,
        ..Default::default()
    });
    breaker.acquire("browser-source").unwrap().record_failure(
        &EngineFailure::new(
            "Browser source",
            "challenge",
            "interactive verification required",
        )
        .with_transient(true),
    );

    assert!(breaker.acquire("browser-source").is_err());
    assert_eq!(breaker.snapshot("browser-source").state, CircuitState::Open);
}

#[test]
fn restored_open_state_preserves_backoff_and_half_open_admission() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        transient_open_duration: Duration::from_millis(20),
        open_backoff_factor: 2,
        open_jitter_ratio: 0.0,
        window: None,
        ..Default::default()
    });
    breaker.restore_open_state("api", Duration::ZERO, 3);

    let probe = breaker
        .acquire("api")
        .expect("an expired restored circuit must admit one probe");
    assert!(breaker.acquire("api").is_err());
    probe.record_failure(&transient_failure());

    let snapshot = breaker.snapshot("api");
    assert_eq!(snapshot.state, CircuitState::Open);
    assert_eq!(snapshot.ejection_count, 4);
    assert!(snapshot.retry_after.is_some_and(|duration| {
        duration > Duration::from_millis(100) && duration <= Duration::from_millis(160)
    }));
}

#[test]
fn transient_failures_use_the_configured_threshold() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 2,
        ..Default::default()
    });
    breaker
        .acquire("http")
        .unwrap()
        .record_failure(&transient_failure());
    assert!(breaker.acquire("http").is_ok());
    breaker
        .acquire("http")
        .unwrap()
        .record_failure(&transient_failure());
    assert!(breaker.acquire("http").is_err());
}

#[test]
fn provider_retry_after_controls_open_duration() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        transient_open_duration: Duration::from_secs(1),
        max_open_duration: Duration::from_secs(60),
        open_jitter_ratio: 0.0,
        ..Default::default()
    });
    let failure = EngineFailure::new("API", "provider_rate_limited", "slow down")
        .with_transient(true)
        .with_retry_after(30);
    breaker.acquire("api").unwrap().record_failure(&failure);

    let retry_after = breaker.acquire("api").unwrap_err().retry_after;
    assert!(retry_after > Duration::from_secs(29));
    assert!(retry_after <= Duration::from_secs(30));
}

#[test]
fn expired_circuit_admits_only_one_half_open_probe() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        transient_open_duration: Duration::ZERO,
        ..Default::default()
    });
    breaker
        .acquire("engine")
        .unwrap()
        .record_failure(&transient_failure());

    let probe = breaker.acquire("engine").unwrap();
    assert_eq!(breaker.snapshot("engine").state, CircuitState::HalfOpen);
    assert!(breaker.acquire("engine").is_err());
    probe.record_success();
    assert_eq!(breaker.snapshot("engine").state, CircuitState::Closed);
    assert!(breaker.acquire("engine").is_ok());
}

#[test]
fn abandoned_half_open_probe_reopens_the_circuit() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        transient_open_duration: Duration::from_millis(1),
        ..Default::default()
    });
    breaker
        .acquire("engine")
        .unwrap()
        .record_failure(&transient_failure());
    std::thread::sleep(Duration::from_millis(2));
    let probe = breaker.acquire("engine").unwrap();
    drop(probe);

    assert_eq!(breaker.snapshot("engine").state, CircuitState::Open);
}

#[test]
fn local_rejection_returns_half_open_probe_without_backoff() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        transient_open_duration: Duration::ZERO,
        open_jitter_ratio: 0.0,
        window: None,
        ..Default::default()
    });
    breaker
        .acquire("engine")
        .unwrap()
        .record_failure(&transient_failure());

    let probe = breaker.acquire("engine").unwrap();
    assert_eq!(breaker.snapshot("engine").state, CircuitState::HalfOpen);
    probe.record_local_rejection();

    let snapshot = breaker.snapshot("engine");
    assert_eq!(snapshot.state, CircuitState::Open);
    assert_eq!(snapshot.ejection_count, 1);
    breaker.acquire("engine").unwrap().record_success();
    assert_eq!(breaker.snapshot("engine").state, CircuitState::Closed);
}

#[test]
fn repeated_empty_results_open_without_affecting_other_engines() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        empty_threshold: 2,
        ..Default::default()
    });
    breaker.acquire("empty").unwrap().record_empty();
    assert!(breaker.acquire("empty").is_ok());
    breaker.acquire("empty").unwrap().record_empty();

    assert!(breaker.acquire("empty").is_err());
    assert!(breaker.acquire("healthy").is_ok());
}

#[test]
fn request_scoped_failures_do_not_poison_later_queries() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        ..Default::default()
    });

    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&EngineFailure::new(
            "API",
            "provider_invalid_request",
            "unsupported option for this request",
        ));

    assert_eq!(breaker.snapshot("api").state, CircuitState::Closed);
    assert_eq!(breaker.snapshot("api").consecutive_failures, 0);
    assert!(breaker.acquire("api").is_ok());
}

#[test]
fn request_scoped_half_open_result_does_not_claim_recovery() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        transient_open_duration: Duration::ZERO,
        ..Default::default()
    });
    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&transient_failure());

    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&EngineFailure::new(
            "API",
            "invalid_query",
            "unsupported query control",
        ));

    assert_eq!(breaker.snapshot("api").state, CircuitState::Open);
}

#[test]
fn sliding_window_opens_on_intermittent_failure_rate() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 10,
        empty_threshold: 10,
        transient_open_duration: Duration::from_secs(60),
        open_jitter_ratio: 0.0,
        window: Some(CircuitWindowConfig {
            size: 4,
            minimum_calls: 4,
            failure_rate_threshold: 0.5,
            slow_call_duration: Duration::from_secs(60),
            slow_call_rate_threshold: 1.0,
        }),
        ..Default::default()
    });

    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&transient_failure());
    breaker.acquire("api").unwrap().record_success();
    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&transient_failure());
    breaker.acquire("api").unwrap().record_success();

    let snapshot = breaker.snapshot("api");
    assert_eq!(snapshot.state, CircuitState::Open);
    assert_eq!(snapshot.recorded_calls, 4);
    assert_eq!(snapshot.failure_rate, Some(0.5));
}

#[test]
fn sliding_window_waits_for_the_minimum_call_count() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 10,
        window: Some(CircuitWindowConfig {
            size: 4,
            minimum_calls: 4,
            failure_rate_threshold: 0.25,
            ..Default::default()
        }),
        ..Default::default()
    });
    for _ in 0..3 {
        breaker
            .acquire("api")
            .unwrap()
            .record_failure(&transient_failure());
    }

    let snapshot = breaker.snapshot("api");
    assert_eq!(snapshot.state, CircuitState::Closed);
    assert_eq!(snapshot.recorded_calls, 3);
    assert_eq!(snapshot.failure_rate, None);
}

#[test]
fn slow_success_rate_can_open_the_circuit() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 10,
        open_jitter_ratio: 0.0,
        window: Some(CircuitWindowConfig {
            size: 4,
            minimum_calls: 4,
            failure_rate_threshold: 1.0,
            slow_call_duration: Duration::from_millis(10),
            slow_call_rate_threshold: 0.5,
        }),
        ..Default::default()
    });

    breaker
        .acquire("api")
        .unwrap()
        .record_success_with_duration(Duration::from_millis(20));
    breaker
        .acquire("api")
        .unwrap()
        .record_success_with_duration(Duration::from_millis(1));
    breaker
        .acquire("api")
        .unwrap()
        .record_success_with_duration(Duration::from_millis(20));
    breaker
        .acquire("api")
        .unwrap()
        .record_success_with_duration(Duration::from_millis(1));

    let snapshot = breaker.snapshot("api");
    assert_eq!(snapshot.state, CircuitState::Open);
    assert_eq!(snapshot.slow_call_rate, Some(0.5));
}

#[test]
fn repeated_half_open_failures_increase_the_bounded_open_duration() {
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        transient_open_duration: Duration::from_millis(20),
        max_open_duration: Duration::from_millis(200),
        open_backoff_factor: 2,
        open_jitter_ratio: 0.0,
        window: None,
        ..Default::default()
    });
    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&transient_failure());
    let first = breaker.acquire("api").unwrap_err().retry_after;
    assert!(first <= Duration::from_millis(20));

    std::thread::sleep(Duration::from_millis(25));
    breaker
        .acquire("api")
        .unwrap()
        .record_failure(&transient_failure());
    let second = breaker.acquire("api").unwrap_err().retry_after;

    assert!(second > Duration::from_millis(30));
    assert!(second <= Duration::from_millis(40));
    assert_eq!(breaker.snapshot("api").ejection_count, 2);
}
