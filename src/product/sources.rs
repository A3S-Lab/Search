//! Product helpers for ACL-aware engine construction.

use std::sync::Arc;

use crate::providers::{BuiltinProvider, ProviderEngine, ProviderReadiness};
use crate::{Engine, EngineConfig, HttpFetcher, PageFetcher, Result, SearchConfig, SearchError};

/// Returns whether ACL enables a shortcut (engines and providers).
pub fn is_config_enabled(config: Option<&SearchConfig>, shortcut: &str) -> bool {
    config
        .and_then(|config| {
            config
                .engine_entry(shortcut)
                .map(|entry| entry.enabled)
                .or_else(|| config.provider_entry(shortcut).map(|entry| entry.enabled))
        })
        .unwrap_or(true)
}

/// Applies ACL engine overrides when present.
pub fn configured_engine_config(
    config: Option<&SearchConfig>,
    engine_config: EngineConfig,
) -> EngineConfig {
    if let Some(config) = config {
        config.apply_engine_config(engine_config)
    } else {
        engine_config
    }
}

/// Builds a built-in provider engine from ACL or environment defaults.
pub fn create_provider_engine(
    provider: BuiltinProvider,
    config: Option<&SearchConfig>,
) -> Result<ProviderEngine> {
    if let Some(config) = config {
        if let Some(engine) = config.create_provider_engine(provider.id())? {
            return Ok(engine);
        }
    }

    let engine = provider.create_engine()?;
    let engine_config = configured_engine_config(config, engine.config().clone());
    Ok(engine.with_config(engine_config))
}

/// Rejects providers that are not ready to make network calls.
pub fn ensure_provider_ready(engine: &ProviderEngine) -> Result<()> {
    match engine.readiness() {
        ProviderReadiness::Ready { .. } => Ok(()),
        ProviderReadiness::MissingCredential {
            environment_variable: Some(variable),
        } => Err(SearchError::Other(format!(
            "{} provider requires credential environment variable {}",
            engine.descriptor().name,
            variable
        ))),
        ProviderReadiness::MissingCredential {
            environment_variable: None,
        } => Err(SearchError::Other(format!(
            "{} provider requires a credential",
            engine.descriptor().name
        ))),
        ProviderReadiness::InvalidCredential => Err(SearchError::Other(format!(
            "{} provider credential is invalid",
            engine.descriptor().name
        ))),
    }
}

/// Builds the shared HTTP fetcher for conventional engines in a tier.
pub fn create_http_fetcher(
    proxy: Option<&str>,
    engine_shortcuts: &[String],
) -> Result<Arc<dyn PageFetcher>> {
    let uses_http_engine = engine_shortcuts.iter().any(|shortcut| {
        matches!(
            shortcut.as_str(),
            "ddg"
                | "duckduckgo"
                | "brave"
                | "bing"
                | "wiki"
                | "wikipedia"
                | "sogou"
                | "360"
                | "so360"
                | "bing_cn"
        )
    });

    let fetcher = match proxy.filter(|_| uses_http_engine) {
        Some(proxy) => HttpFetcher::with_proxy(proxy)?,
        None => HttpFetcher::new(),
    };
    Ok(Arc::new(fetcher))
}
