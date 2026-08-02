//! Structurally gated CLI search cascade.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;

use a3s_search::{
    engines::{
        Bing, BingChina, BingParser, Brave, BraveParser, DuckDuckGo, DuckDuckGoParser, So360,
        So360Parser, Sogou, SogouParser, Wikipedia,
    },
    providers::BuiltinProvider,
    Bulkhead, CircuitBreaker, Engine, EngineFailure, HttpFetcher, PageFetcher,
    RetrievalRequirements, Search, SearchCascade, SearchCascadeOutcomeV2, SearchCoalescer,
    SearchConfig, SearchError, SearchQuery, SearchResults,
};

use super::provider::{create_provider_engine, ensure_provider_ready};
use super::proxy::create_http_fetcher;
use crate::{configured_engine_config, is_config_enabled};

mod browser;
mod plan;

use browser::execute_headless_tier;
pub(crate) use browser::HeadlessBrowser;
pub(crate) use plan::{EngineTier, EngineTierPlan};

/// Default end-to-end CLI deadline when ACL and command-line options omit one.
pub(crate) const DEFAULT_TIMEOUT_SECONDS: u64 = 20;

/// Inputs that are shared across all lazily executed tiers.
pub(crate) struct CascadeRequest<'a> {
    pub query: SearchQuery,
    pub limit: usize,
    pub timeout: Duration,
    pub proxy: Option<&'a str>,
    pub config: Option<&'a SearchConfig>,
    pub browser: HeadlessBrowser,
    pub browser_max_retries: u32,
}

#[derive(Clone, Default)]
struct SharedControls {
    circuit: CircuitBreaker,
    bulkhead: Bulkhead,
    coalescer: SearchCoalescer,
}

/// Executes a lazy retrieval cascade and returns structurally bound results.
pub(crate) async fn execute_cascade(
    plan: &EngineTierPlan,
    request: CascadeRequest<'_>,
    circuit: CircuitBreaker,
) -> Result<SearchCascadeOutcomeV2> {
    let deadline = Instant::now()
        .checked_add(request.timeout)
        .ok_or_else(|| anyhow::anyhow!("search timeout exceeds the platform limit"))?;
    let requirements = RetrievalRequirements::for_limit(request.limit);
    let mut cascade = SearchCascade::new(request.query.clone(), requirements);
    let controls = SharedControls {
        circuit,
        ..SharedControls::default()
    };
    let tiers = plan.tiers();
    let configured_tiers = tiers
        .iter()
        .map(|(tier, _)| tier.receipt_name())
        .collect::<Vec<_>>();

    for (index, (tier, shortcuts)) in tiers.iter().copied().enumerate() {
        if !cascade.needs_next_tier() {
            break;
        }
        let remaining_tiers = tiers.len().saturating_sub(index + 1);
        let results = match tier {
            EngineTier::Headless => {
                execute_headless_tier(&request, &controls, shortcuts, deadline, remaining_tiers)
                    .await
            }
            EngineTier::HttpRss | EngineTier::Api => {
                execute_network_tier(
                    &request,
                    &controls,
                    shortcuts,
                    tier,
                    deadline,
                    remaining_tiers,
                )
                .await?
            }
        };
        cascade.push_tier(tier.receipt_name(), results);
    }

    let outcome = cascade.finish_with_tier_plan(configured_tiers)?;
    outcome.validate()?;
    Ok(outcome)
}

fn configured_search(config: Option<&SearchConfig>, controls: &SharedControls) -> Search {
    let mut search = if let Some(config) = config {
        Search::with_health_config(config.health_config())
    } else {
        Search::new()
    }
    .with_circuit_breaker(controls.circuit.clone())
    .with_bulkhead(controls.bulkhead.clone())
    .with_request_coalescer(controls.coalescer.clone());
    if let Some(config) = config {
        search.set_ranking_config(config.ranking);
    }
    search
}

async fn execute_network_tier(
    request: &CascadeRequest<'_>,
    controls: &SharedControls,
    shortcuts: &[String],
    tier: EngineTier,
    deadline: Instant,
    remaining_tiers: usize,
) -> Result<SearchResults> {
    let mut search = configured_search(request.config, controls);
    let mut setup_results = SearchResults::new();
    if tier == EngineTier::Api {
        for shortcut in shortcuts {
            if !record_disabled_engine(&mut setup_results, request.config, shortcut) {
                continue;
            }
            add_provider_engine(&mut search, &mut setup_results, shortcut, request.config);
        }
    } else {
        let http_fetcher = create_http_fetcher(request.proxy, shortcuts)?;
        for shortcut in shortcuts {
            if !record_disabled_engine(&mut setup_results, request.config, shortcut) {
                continue;
            }
            add_http_engine(
                &mut search,
                &mut setup_results,
                shortcut,
                request.proxy,
                &http_fetcher,
                request.config,
            );
        }
    }

    let remaining = deadline.saturating_duration_since(Instant::now());
    let budget = tier_timeout(remaining, remaining_tiers);
    Ok(execute_search_tier(
        search,
        setup_results,
        &request.query,
        tier.receipt_name(),
        budget,
    )
    .await)
}

fn record_disabled_engine(
    results: &mut SearchResults,
    config: Option<&SearchConfig>,
    shortcut: &str,
) -> bool {
    if is_config_enabled(config, shortcut) {
        return true;
    }
    results.add_failure(EngineFailure::new(
        shortcut,
        "engine_disabled",
        "engine is disabled by ACL configuration",
    ));
    false
}

fn add_provider_engine(
    search: &mut Search,
    results: &mut SearchResults,
    shortcut: &str,
    config: Option<&SearchConfig>,
) {
    let Some(provider) = BuiltinProvider::from_id(shortcut) else {
        results.add_failure(EngineFailure::new(
            shortcut,
            "unsupported_engine",
            "engine is not a built-in API provider",
        ));
        return;
    };
    match create_provider_engine(provider, config) {
        Ok(engine) => match ensure_provider_ready(&engine) {
            Ok(()) => search.add_engine(engine),
            Err(error) => results.add_failure(EngineFailure::new(
                shortcut,
                "provider_not_ready",
                error.to_string(),
            )),
        },
        Err(error) => results.add_failure(EngineFailure::new(
            shortcut,
            "provider_configuration",
            error.to_string(),
        )),
    }
}

fn add_http_engine(
    search: &mut Search,
    results: &mut SearchResults,
    shortcut: &str,
    proxy: Option<&str>,
    fetcher: &Arc<dyn PageFetcher>,
    config: Option<&SearchConfig>,
) {
    macro_rules! add_html_engine {
        ($engine:expr) => {{
            let engine = $engine;
            let engine_config = configured_engine_config(config, engine.config().clone());
            search.add_engine(engine.with_config(engine_config));
        }};
    }

    match shortcut {
        "ddg" => add_html_engine!(DuckDuckGo::with_fetcher(
            DuckDuckGoParser,
            Arc::clone(fetcher)
        )),
        "brave" => add_html_engine!(Brave::with_fetcher(BraveParser, Arc::clone(fetcher))),
        "bing" => add_html_engine!(Bing::with_fetcher(BingParser, Arc::clone(fetcher))),
        "wiki" => {
            let wiki_fetcher = match proxy {
                Some(proxy) => HttpFetcher::with_proxy(proxy),
                None => Ok(HttpFetcher::new()),
            };
            match wiki_fetcher {
                Ok(wiki_fetcher) => {
                    let engine = Wikipedia::with_http_fetcher(wiki_fetcher);
                    let engine_config = configured_engine_config(config, engine.config().clone());
                    search.add_engine(engine.with_config(engine_config));
                }
                Err(error) => results.add_failure(EngineFailure::new(
                    shortcut,
                    error.kind(),
                    error.to_string(),
                )),
            }
        }
        "sogou" => add_html_engine!(Sogou::with_fetcher(SogouParser, Arc::clone(fetcher))),
        "360" => add_html_engine!(So360::with_fetcher(So360Parser, Arc::clone(fetcher))),
        "bing_cn" => add_html_engine!(BingChina::new(Arc::clone(fetcher))),
        _ => results.add_failure(EngineFailure::new(
            shortcut,
            "unsupported_engine",
            "engine is not available in the HTTP/RSS tier",
        )),
    }
}

async fn execute_search_tier(
    mut search: Search,
    mut setup_results: SearchResults,
    query: &SearchQuery,
    tier: &str,
    budget: Duration,
) -> SearchResults {
    if search.engine_count() == 0 {
        return setup_results;
    }
    if budget.is_zero() {
        setup_results.merge(deadline_exhausted(tier));
        return setup_results;
    }

    search.set_timeout(
        budget
            .saturating_sub(Duration::from_millis(100))
            .max(Duration::from_millis(1)),
    );
    match tokio::time::timeout(budget, search.search(query.clone())).await {
        Ok(Ok(results)) => setup_results.merge(results),
        Ok(Err(error)) => setup_results.add_failure(search_error_failure(tier, &error)),
        Err(_) => setup_results.add_failure(
            EngineFailure::new(tier, "timeout", "search tier timed out").with_transient(true),
        ),
    }
    setup_results
}

fn tier_timeout(remaining: Duration, remaining_tiers: usize) -> Duration {
    let divisor = u128::try_from(remaining_tiers)
        .unwrap_or(u128::MAX)
        .saturating_add(1);
    let milliseconds = (remaining.as_millis() / divisor).max(1);
    Duration::from_millis(u64::try_from(milliseconds).unwrap_or(u64::MAX)).min(remaining)
}

fn deadline_exhausted(tier: &str) -> SearchResults {
    let mut results = SearchResults::new();
    results.add_failure(
        EngineFailure::new(
            tier,
            "timeout",
            "search deadline was exhausted before this tier could start",
        )
        .with_transient(true),
    );
    results
}

fn search_error_failure(tier: &str, error: &SearchError) -> EngineFailure {
    let mut failure = EngineFailure::new(tier, error.kind(), error.to_string())
        .with_transient(error.is_transient());
    if let SearchError::Provider(provider) = error {
        failure = failure.with_provider(provider.provider());
    }
    if let Some(seconds) = error.retry_after_seconds() {
        failure = failure.with_retry_after(seconds);
    }
    failure
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_deadline_is_shared_fairly_across_remaining_tiers() {
        assert_eq!(
            tier_timeout(Duration::from_secs(30), 2),
            Duration::from_secs(10)
        );
        assert_eq!(
            tier_timeout(Duration::from_secs(7), 0),
            Duration::from_secs(7)
        );
    }

    #[tokio::test]
    async fn timeout_that_cannot_fit_an_instant_is_rejected() {
        let plan = EngineTierPlan::new(Some(&["ddg".to_string()]), None, None).unwrap();
        let error = execute_cascade(
            &plan,
            CascadeRequest {
                query: SearchQuery::new("portable query"),
                limit: 1,
                timeout: Duration::MAX,
                proxy: None,
                config: None,
                browser: HeadlessBrowser::Chrome,
                browser_max_retries: 1,
            },
            CircuitBreaker::default(),
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("search timeout exceeds the platform limit"));
    }
}
