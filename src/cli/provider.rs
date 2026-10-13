//! Built-in provider construction and readiness display.

use std::path::Path;

use anyhow::Result;

use a3s_search::product::create_provider_engine as product_create_provider_engine;
use a3s_search::{
    providers::{BuiltinProvider, ProviderAuthentication, ProviderEngine, ProviderReadiness},
    SearchConfig,
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
        println!("  Headless (uses Moli by default; Chrome/Chromium and Lightpanda are explicit):");
        println!("    brave_browser - Brave Search (built-in default)");
        println!("    bing_browser  - Bing International");
        println!("    g             - Google");
        println!("    baidu         - Baidu (百度)");
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
            "    {:<12} - {} ({})",
            provider.id(),
            descriptor.name,
            status
        );
    }

    println!();
    println!(
        "Usage: a3s-search \"query\" -e ddg,wiki,anysearch,tavily,tinyfish,bocha,aliyun,tencent,firecrawl,a3s"
    );
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
    product_create_provider_engine(provider, config).map_err(anyhow::Error::from)
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
