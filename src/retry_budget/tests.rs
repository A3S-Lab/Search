use super::*;

#[test]
fn retries_consume_a_bounded_shared_reserve() {
    let budget = RetryBudget::new(RetryBudgetConfig {
        capacity: 10,
        retry_cost: 5,
        success_credit: 1,
    });
    let shared = budget.clone();

    assert!(budget.try_acquire_retry());
    assert!(shared.try_acquire_retry());
    assert!(!budget.try_acquire_retry());
    assert_eq!(budget.snapshot().available, 0);
    assert_eq!(budget.snapshot().admitted_retries, 2);
    assert_eq!(budget.snapshot().rejected_retries, 1);
}

#[test]
fn successes_replenish_only_to_the_configured_capacity() {
    let budget = RetryBudget::new(RetryBudgetConfig {
        capacity: 10,
        retry_cost: 5,
        success_credit: 2,
    });
    assert!(budget.try_acquire_retry());
    for _ in 0..20 {
        budget.record_success();
    }

    assert_eq!(budget.snapshot().available, 10);
    assert!(budget.try_acquire_retry());
}

#[test]
fn zero_capacity_disables_retries_without_panicking() {
    let budget = RetryBudget::new(RetryBudgetConfig {
        capacity: 0,
        retry_cost: 0,
        success_credit: u64::MAX,
    });

    assert!(!budget.try_acquire_retry());
    budget.record_success();
    assert_eq!(budget.snapshot().available, 0);
}
