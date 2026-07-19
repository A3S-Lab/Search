//! Built-in provider construction and readiness display.

use std::path::Path;

use anyhow::Result;

use a3s_search::{
    providers::{BuiltinProvider, ProviderAuthentication, ProviderEngine, ProviderReadiness},
    Engine, SearchConfig,
};

pub(crate) fn list_engines(config_path: Option<&Path>) -> Result<()> {
    let config = load_search_config(config_path)?;

    println!("Available search engines:\n");
    println!("  International:");
    println!("    ddg      - DuckDuckGo (privacy-focused search)");
    println!("    brave    - Brave Search");
    println!("    bing     - Bing International");
    println!("    wiki     - Wikipedia");
    println!();
    println!("  Chinese:");
    println!("    sogou    - Sogou (搜狗)");
    println!("    360      - 360 Search (360搜索)");
    println!("    bing_cn  - Bing China (必应中国)");

    #[cfg(feature = "headless")]
    {
        println!();
        println!("  Headless (uses an installed Chrome/Chromium):");
        println!("    g        - Google");
        println!("    baidu    - Baidu (百度)");
    }

    println!();
    println!("  Native API providers:");
    for provider in BuiltinProvider::ALL {
        let engine = create_provider_engine(provider, config.as_ref())?;
        let descriptor = engine.descriptor();
        let enabled = config
            .as_ref()
            .and_then(|config| config.provider_entry(provider.id()))
            .map(|entry| entry.enabled)
            .unwrap_or(true);
        let status = if enabled {
            provider_readiness_summary(&engine.readiness())
        } else {
            "disabled by config".to_string()
        };
        println!(
            "    {:<10} - {} ({})",
            provider.id(),
            descriptor.name,
            status
        );
    }

    println!();
    println!("Usage: a3s-search \"query\" -e ddg,wiki,anysearch,tavily");
    Ok(())
}

pub(crate) fn load_search_config(path: Option<&Path>) -> Result<Option<SearchConfig>> {
    path.map(|path| {
        SearchConfig::load(path)
            .map_err(|error| anyhow::anyhow!("Failed to load config {}: {}", path.display(), error))
    })
    .transpose()
}

pub(crate) fn create_provider_engine(
    provider: BuiltinProvider,
    config: Option<&SearchConfig>,
) -> Result<ProviderEngine> {
    if let Some(config) = config {
        if let Some(engine) = config.create_provider_engine(provider.id())? {
            return Ok(engine);
        }
    }

    let engine = provider.create_engine()?;
    let engine_config = super::super::configured_engine_config(config, engine.config().clone());
    Ok(engine.with_config(engine_config))
}

pub(crate) fn provider_readiness_summary(readiness: &ProviderReadiness) -> String {
    match readiness {
        ProviderReadiness::Ready {
            authentication: ProviderAuthentication::Anonymous,
        } => "ready, keyless/anonymous".to_string(),
        ProviderReadiness::Ready {
            authentication: ProviderAuthentication::Authenticated,
        } => "ready, authenticated".to_string(),
        ProviderReadiness::Ready { .. } => "ready".to_string(),
        ProviderReadiness::MissingCredential {
            environment_variable: Some(variable),
        } => format!("not ready, missing {variable}"),
        ProviderReadiness::MissingCredential {
            environment_variable: None,
        } => "not ready, missing credential".to_string(),
        ProviderReadiness::InvalidCredential => "not ready, invalid credential".to_string(),
        _ => "not ready, unsupported readiness state".to_string(),
    }
}

pub(crate) fn ensure_provider_ready(engine: &ProviderEngine) -> Result<()> {
    match engine.readiness() {
        ProviderReadiness::Ready { .. } => Ok(()),
        ProviderReadiness::MissingCredential {
            environment_variable: Some(variable),
        } => anyhow::bail!(
            "{} provider requires credential environment variable {}",
            engine.descriptor().name,
            variable
        ),
        ProviderReadiness::MissingCredential {
            environment_variable: None,
        } => anyhow::bail!(
            "{} provider requires a credential",
            engine.descriptor().name
        ),
        ProviderReadiness::InvalidCredential => {
            anyhow::bail!(
                "{} provider credential is invalid",
                engine.descriptor().name
            )
        }
        _ => anyhow::bail!(
            "{} provider returned an unsupported readiness state",
            engine.descriptor().name
        ),
    }
}
