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
