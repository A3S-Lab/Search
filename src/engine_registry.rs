//! Immutable engine registrations and request-time admission.
//!
//! The public [`Engine`] trait intentionally remains small and object-safe.
//! Once an engine is added to [`crate::Search`], this module snapshots its
//! configuration so request execution has one stable timeout/category/weight
//! descriptor. Trait-provided identity is captured into each admitted
//! attempt, so custom `Engine` implementations remain compatible while one
//! request still has a consistent name and shortcut. Runtime policy (health
//! and the shared circuit) is consulted only during selection; the registry
//! itself owns no mutable request state.

use std::sync::Arc;
use std::time::Duration;

use tracing::debug;

use crate::{
    CircuitBreaker, CircuitPermit, Engine, EngineConfig, EngineFailure, EngineOutcome,
    EngineOutcomeKind, HealthMonitor, SearchQuery,
};

/// One engine and the configuration captured when it was registered.
#[derive(Clone)]
pub(crate) struct RegisteredEngine {
    pub(crate) engine: Arc<dyn Engine>,
    pub(crate) config: Arc<EngineConfig>,
}

impl RegisteredEngine {
    fn new<E: Engine + 'static>(engine: E) -> Self {
        let config = engine.config().clone();
        Self {
            engine: Arc::new(engine),
            config: Arc::new(config),
        }
    }
}

/// An engine admitted for execution, including its optional circuit token.
pub(crate) struct EngineAttempt {
    pub(crate) engine: RegisteredEngine,
    /// Identity captured together with admission so execution cannot observe
    /// a different trait-provided name or shortcut halfway through a request.
    pub(crate) name: String,
    pub(crate) shortcut: String,
    pub(crate) circuit_permit: Option<CircuitPermit>,
}

/// Engines skipped before an upstream call and engines ready to run.
pub(crate) struct EngineSelection {
    pub(crate) attempts: Vec<EngineAttempt>,
    pub(crate) skipped_outcomes: Vec<EngineOutcome>,
    pub(crate) skipped_failures: Vec<EngineFailure>,
}

/// Registry of caller-owned search engines.
#[derive(Default)]
pub(crate) struct EngineRegistry {
    engines: Vec<RegisteredEngine>,
}

impl EngineRegistry {
    /// Registers an engine and returns its immutable configuration snapshot.
    pub(crate) fn add<E: Engine + 'static>(&mut self, engine: E) -> EngineConfig {
        let registered = RegisteredEngine::new(engine);
        let config = registered.config.as_ref().clone();
        self.engines.push(registered);
        config
    }

    pub(crate) fn len(&self) -> usize {
        self.engines.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.engines.is_empty()
    }

    /// Returns the configurations used to build a coalescing identity.
    pub(crate) fn configs(&self) -> impl Iterator<Item = &EngineConfig> {
        self.engines.iter().map(|engine| engine.config.as_ref())
    }

    /// Selects enabled engines matching the query and admits their circuits.
    pub(crate) fn select(
        &self,
        query: &SearchQuery,
        health: Option<&HealthMonitor>,
        circuit_breaker: Option<&CircuitBreaker>,
    ) -> EngineSelection {
        let mut attempts = Vec::with_capacity(self.engines.len());
        let mut skipped_outcomes = Vec::with_capacity(self.engines.len());
        let mut skipped_failures = Vec::with_capacity(self.engines.len());

        for engine in &self.engines {
            if !engine.engine.is_enabled() {
                continue;
            }
            let Some(shortcut) = shortcut_for_query(engine, query) else {
                continue;
            };
            let name = engine.engine.name().to_string();

            if health.is_some_and(|monitor| monitor.is_suspended(&name)) {
                debug!(engine = %name, "engine is suspended, skipping");
                let failure = EngineFailure::new(
                    name.clone(),
                    "engine_suspended",
                    "local engine health monitor is open",
                )
                .with_transient(true);
                skipped_outcomes.push(EngineOutcome::failed(
                    shortcut.clone(),
                    failure.clone(),
                    EngineOutcomeKind::CircuitOpen,
                ));
                skipped_failures.push(failure);
                continue;
            }

            let circuit_permit = match circuit_breaker {
                None => None,
                Some(circuit) => match circuit.acquire(&shortcut) {
                    Ok(permit) => Some(permit),
                    Err(open) => {
                        let retry_after_seconds = duration_ceiling_seconds(open.retry_after);
                        let mut failure = EngineFailure::new(
                            name.clone(),
                            "circuit_open",
                            "shared engine circuit is open",
                        )
                        .with_transient(true);
                        if retry_after_seconds > 0 {
                            failure = failure.with_retry_after(retry_after_seconds);
                        }
                        skipped_outcomes.push(EngineOutcome::failed(
                            shortcut.clone(),
                            failure.clone(),
                            EngineOutcomeKind::CircuitOpen,
                        ));
                        skipped_failures.push(failure);
                        continue;
                    }
                },
            };

            attempts.push(EngineAttempt {
                engine: engine.clone(),
                name,
                shortcut,
                circuit_permit,
            });
        }

        EngineSelection {
            attempts,
            skipped_outcomes,
            skipped_failures,
        }
    }
}

fn shortcut_for_query(engine: &RegisteredEngine, query: &SearchQuery) -> Option<String> {
    if !query.engines.is_empty() {
        let shortcut = engine.engine.shortcut().to_string();
        return query
            .engines
            .iter()
            .any(|candidate| candidate == &shortcut)
            .then_some(shortcut);
    }
    let matches_category = query
        .categories
        .iter()
        .any(|category| engine.config.categories.contains(category));
    matches_category.then(|| engine.engine.shortcut().to_string())
}

fn duration_ceiling_seconds(duration: Duration) -> u64 {
    let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    millis.saturating_add(999) / 1_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineCategory, SearchResult};
    use async_trait::async_trait;

    struct TestEngine {
        config: EngineConfig,
    }

    #[async_trait]
    impl Engine for TestEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> crate::Result<Vec<SearchResult>> {
            Ok(Vec::new())
        }
    }

    struct TraitOverrideEngine {
        config: EngineConfig,
        name: String,
        shortcut: String,
        enabled: bool,
    }

    #[async_trait]
    impl Engine for TraitOverrideEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn shortcut(&self) -> &str {
            &self.shortcut
        }

        fn is_enabled(&self) -> bool {
            self.enabled
        }

        async fn search(&self, _query: &SearchQuery) -> crate::Result<Vec<SearchResult>> {
            Ok(Vec::new())
        }
    }

    fn engine(name: &str, shortcut: &str, category: EngineCategory) -> TestEngine {
        TestEngine {
            config: EngineConfig {
                name: name.to_string(),
                shortcut: shortcut.to_string(),
                categories: vec![category],
                ..EngineConfig::default()
            },
        }
    }

    #[test]
    fn registration_returns_a_configuration_snapshot() {
        let mut registry = EngineRegistry::default();
        let config = registry.add(engine("stable", "stable", EngineCategory::General));

        assert_eq!(config.name, "stable");
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.configs().next().map(|value| value.name.as_str()),
            Some("stable")
        );
    }

    #[test]
    fn selection_respects_explicit_shortcuts_and_categories() {
        let mut registry = EngineRegistry::default();
        registry.add(engine("general", "general", EngineCategory::General));
        registry.add(engine("images", "images", EngineCategory::Images));

        let selection = registry.select(
            &SearchQuery::new("query").with_categories(vec![EngineCategory::Images]),
            None,
            None,
        );
        assert_eq!(selection.attempts.len(), 1);
        assert_eq!(selection.attempts[0].engine.config.shortcut, "images");

        let selection = registry.select(
            &SearchQuery::new("query").with_engines(vec!["general".to_string()]),
            None,
            None,
        );
        assert_eq!(selection.attempts.len(), 1);
        assert_eq!(selection.attempts[0].engine.config.shortcut, "general");
    }

    #[test]
    fn selection_preserves_overridden_engine_trait_identity() {
        let mut registry = EngineRegistry::default();
        registry.add(TraitOverrideEngine {
            config: EngineConfig {
                name: "config-name".to_string(),
                shortcut: "config-shortcut".to_string(),
                categories: vec![EngineCategory::General],
                ..EngineConfig::default()
            },
            name: "runtime-name".to_string(),
            shortcut: "runtime-shortcut".to_string(),
            enabled: true,
        });

        let selection = registry.select(
            &SearchQuery::new("query").with_engines(vec!["runtime-shortcut".to_string()]),
            None,
            None,
        );
        assert_eq!(selection.attempts.len(), 1);
        assert_eq!(selection.attempts[0].name, "runtime-name");
        assert_eq!(selection.attempts[0].shortcut, "runtime-shortcut");
    }
}
