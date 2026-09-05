//! Execution boundary for one admitted search-engine attempt.
//!
//! A runner owns the mechanics that must be identical for every source:
//! bounded local admission, the per-engine deadline, metrics, typed outcome
//! construction, and circuit completion.  The [`crate::Search`] coordinator
//! only schedules runners and folds their independent results together.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::time::timeout;
use tracing::{debug, warn};

use crate::engine_registry::EngineAttempt;
use crate::{
    Bulkhead, EngineFailure, EngineOutcome, EngineOutcomeKind, EngineOutput, Metrics, SearchError,
    SearchQuery,
};

/// Result of one engine attempt, including enough information for health
/// accounting and response assembly.
pub(crate) enum EngineExecution {
    /// The engine returned a structurally valid output.
    Completed {
        engine_name: String,
        output: EngineOutput,
        outcome: EngineOutcome,
    },
    /// The attempt did not produce output.
    Failed {
        failure: EngineFailure,
        affects_health: bool,
        outcome: EngineOutcome,
    },
}

/// Shared execution policy for all engines in one [`crate::Search`] request.
#[derive(Clone, Default)]
pub(crate) struct EngineRunner {
    timeout_override: Option<Duration>,
    metrics: Option<Arc<Metrics>>,
    bulkhead: Option<Bulkhead>,
}

impl EngineRunner {
    pub(crate) fn new(
        timeout_override: Option<Duration>,
        metrics: Option<Arc<Metrics>>,
        bulkhead: Option<Bulkhead>,
    ) -> Self {
        Self {
            timeout_override,
            metrics,
            bulkhead,
        }
    }

    /// Executes one admitted engine attempt and consumes its circuit permit.
    pub(crate) async fn run(
        &self,
        attempt: EngineAttempt,
        query: Arc<SearchQuery>,
    ) -> EngineExecution {
        let registered = attempt.engine;
        let engine = registered.engine;
        let config = registered.config;
        let name = attempt.name;
        let shortcut = attempt.shortcut;
        let timeout_duration = self
            .timeout_override
            .unwrap_or_else(|| Duration::from_secs(config.timeout));
        let started = Instant::now();
        let mut circuit_permit = attempt.circuit_permit;

        let _bulkhead_permit = match self.bulkhead.clone() {
            None => None,
            Some(bulkhead) => match bulkhead.acquire(&shortcut).await {
                Ok(permit) => Some(permit),
                Err(rejection) => {
                    if let Some(permit) = circuit_permit.take() {
                        // Local overload did not reach the upstream.  A
                        // half-open probe must remain eligible immediately.
                        permit.record_local_rejection();
                    }
                    self.record_failure(rejection.failure_kind(), true);
                    let failure = EngineFailure::new(
                        name.clone(),
                        rejection.failure_kind(),
                        rejection.to_string(),
                    )
                    .with_transient(true);
                    let outcome = EngineOutcome::failed(
                        shortcut,
                        failure.clone(),
                        EngineOutcomeKind::Rejected,
                    )
                    .with_duration(started.elapsed());
                    return EngineExecution::Failed {
                        failure,
                        affects_health: false,
                        outcome,
                    };
                }
            },
        };

        match timeout(timeout_duration, engine.search_output(&query)).await {
            Ok(Ok(output)) => {
                let duration = started.elapsed();
                self.record_success(duration);
                debug!(engine = %name, results = output.results.len(), "engine completed");
                let outcome_kind = if output.is_empty() {
                    if let Some(permit) = circuit_permit.take() {
                        permit.record_empty_with_duration(duration);
                    }
                    EngineOutcomeKind::Empty
                } else {
                    if let Some(permit) = circuit_permit.take() {
                        permit.record_success_with_duration(duration);
                    }
                    EngineOutcomeKind::Success
                };
                let mut outcome = EngineOutcome::completed(
                    name.clone(),
                    shortcut,
                    outcome_kind,
                    output.results.len(),
                )
                .with_duration(duration);
                outcome.provider = output.provider();
                EngineExecution::Completed {
                    engine_name: name,
                    output,
                    outcome,
                }
            }
            Ok(Err(error)) => {
                let duration = started.elapsed();
                self.record_failure(error.kind(), error.is_transient());
                warn!(engine = %name, error = %error, "engine failed");
                let affects_health = !error.is_client_error();
                let failure = EngineFailure::from_search_error(name, &error);
                if let Some(permit) = circuit_permit.take() {
                    permit.record_failure_with_duration(&failure, duration);
                }
                let outcome =
                    EngineOutcome::failed(shortcut, failure.clone(), EngineOutcomeKind::Failure)
                        .with_duration(duration);
                EngineExecution::Failed {
                    failure,
                    affects_health,
                    outcome,
                }
            }
            Err(_) => {
                let duration = started.elapsed();
                self.record_failure(SearchError::Timeout.kind(), true);
                warn!(engine = %name, "engine timed out");
                let failure = EngineFailure::new(name, "timeout", "timed out").with_transient(true);
                if let Some(permit) = circuit_permit.take() {
                    permit.record_failure_with_duration(&failure, duration);
                }
                let outcome =
                    EngineOutcome::failed(shortcut, failure.clone(), EngineOutcomeKind::Timeout)
                        .with_duration(duration);
                EngineExecution::Failed {
                    failure,
                    affects_health: true,
                    outcome,
                }
            }
        }
    }

    fn record_success(&self, duration: Duration) {
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.record_success(duration);
        }
    }

    fn record_failure(&self, kind: &str, transient: bool) {
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.record_failure(kind, transient);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, EngineCategory, EngineConfig, EngineOutput, Result, SearchResult};
    use async_trait::async_trait;

    struct EmptyEngine {
        config: EngineConfig,
    }

    #[async_trait]
    impl Engine for EmptyEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            Ok(Vec::new())
        }

        async fn search_output(&self, _query: &SearchQuery) -> Result<EngineOutput> {
            Ok(EngineOutput::default())
        }
    }

    #[tokio::test]
    async fn runner_classifies_empty_output_without_treating_it_as_failure() {
        let mut registry = crate::engine_registry::EngineRegistry::default();
        registry.add(EmptyEngine {
            config: EngineConfig {
                name: "empty".to_string(),
                shortcut: "empty".to_string(),
                categories: vec![EngineCategory::General],
                ..EngineConfig::default()
            },
        });
        let selection = registry.select(&SearchQuery::new("query"), None, None);
        let execution = EngineRunner::default()
            .run(
                selection.attempts.into_iter().next().expect("attempt"),
                Arc::new(SearchQuery::new("query")),
            )
            .await;

        match execution {
            EngineExecution::Completed { outcome, .. } => {
                assert_eq!(outcome.kind, EngineOutcomeKind::Empty);
            }
            EngineExecution::Failed { .. } => panic!("empty output must complete"),
        }
    }
}
