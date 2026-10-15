//! Library-owned product cascade execution.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::engines::{
    Bing, BingChina, BingParser, Brave, BraveParser, DuckDuckGo, DuckDuckGoParser, So360,
    So360Parser, Sogou, SogouParser, Wikipedia,
};
use crate::product::browser::{self, HeadlessBrowser, PendingHeadlessRuntime};
use crate::product::sources::{
    configured_engine_config, create_http_fetcher, create_provider_engine, ensure_provider_ready,
    is_config_enabled,
};
use crate::providers::BuiltinProvider;
use crate::retry::RetryBudget;
use crate::{
    Bulkhead, CircuitBreaker, Engine, EngineFailure, EngineTier, EngineTierPlan, HttpFetcher,
    Metrics, PageFetcher, RetrievalRequirements, Search, SearchCascade, SearchCascadeOutcomeV2,
    SearchCoalescer, SearchConfig, SearchError, SearchQuery, SearchResult, SearchResults,
};

/// Default end-to-end deadline when ACL and callers omit one.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 20;

/// Inputs shared across lazily executed tiers.
pub struct CascadeRequest<'a> {
    pub query: SearchQuery,
    pub limit: usize,
    pub timeout: Duration,
    pub proxy: Option<&'a str>,
    pub config: Option<&'a SearchConfig>,
    pub browser: HeadlessBrowser,
    pub browser_max_retries: u32,
}

/// Decides whether a host can use a result; rejected rows never count toward
/// the structural gate.
pub type ResultAdmission = Arc<dyn Fn(&SearchResult) -> bool + Send + Sync>;

/// Shared reliability controls and host-owned resources for one cascade run.
#[derive(Clone, Default)]
pub struct CascadeControls {
    pub circuit: CircuitBreaker,
    pub bulkhead: Bulkhead,
    pub coalescer: SearchCoalescer,
    /// Host metrics sink shared by every tier.
    pub metrics: Option<Arc<Metrics>>,
    /// Host retry budget for browser renders; a fresh budget per run when unset.
    pub retry_budget: Option<RetryBudget>,
    /// Browser runtime the host prepares; awaited only when the headless tier runs.
    pub headless_runtime: Option<PendingHeadlessRuntime>,
    pub admission: Option<ResultAdmission>,
}

impl CascadeControls {
    /// Creates controls around a shared circuit breaker.
    pub fn with_circuit(circuit: CircuitBreaker) -> Self {
        Self {
            circuit,
            ..Self::default()
        }
    }
}

/// Executes a lazy retrieval cascade and returns structurally bound results.
pub async fn execute_cascade(
    plan: &EngineTierPlan,
    request: CascadeRequest<'_>,
    controls: CascadeControls,
) -> Result<SearchCascadeOutcomeV2, SearchError> {
    let deadline = Instant::now().checked_add(request.timeout).ok_or_else(|| {
        SearchError::Other("search timeout exceeds the platform limit".to_string())
    })?;
    let requirements = RetrievalRequirements::for_limit(request.limit);
    let mut cascade = SearchCascade::new(request.query.clone(), requirements);
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
        let mut results = match tier {
            EngineTier::Headless => {
                browser::execute_headless_tier(
                    &request,
                    &controls,
                    shortcuts,
                    deadline,
                    remaining_tiers,
                )
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
        admit_results(&mut results, controls.admission.as_ref());
        cascade.push_tier(tier.receipt_name(), results);
    }

    cascade
        .finish_with_tier_plan(configured_tiers)
        .map_err(|error| SearchError::Other(error.to_string()))
}

fn admit_results(results: &mut SearchResults, admission: Option<&ResultAdmission>) {
    if let Some(admit) = admission {
        results.items_mut().retain(|result| admit(result));
        results.count = results.items().len();
    }
}

pub(crate) fn configured_search(
    config: Option<&SearchConfig>,
    controls: &CascadeControls,
) -> Search {
    // Product cascade attaches a shared circuit, so local health is not the
    // isolation owner. Ranking still comes from ACL when present.
    let mut search = Search::new()
        .with_circuit_breaker(controls.circuit.clone())
        .with_bulkhead(controls.bulkhead.clone())
        .with_request_coalescer(controls.coalescer.clone());
    if let Some(metrics) = controls.metrics.as_ref() {
        search = search.with_metrics(Arc::clone(metrics));
    }
    if let Some(config) = config {
        search.set_ranking_config(config.ranking);
    }
    search
}

async fn execute_network_tier(
    request: &CascadeRequest<'_>,
    controls: &CascadeControls,
    shortcuts: &[String],
    tier: EngineTier,
    deadline: Instant,
    remaining_tiers: usize,
) -> Result<SearchResults, SearchError> {
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

pub(crate) fn record_disabled_engine(
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
            Ok(()) => {
                search.add_engine(engine);
            }
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

pub(crate) async fn execute_search_tier(
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
        Ok(Err(error)) => setup_results.add_failure(EngineFailure::from_search_error(tier, &error)),
        Err(_) => setup_results.add_failure(
            EngineFailure::new(tier, "timeout", "search tier timed out").with_transient(true),
        ),
    }
    setup_results
}

pub(crate) fn tier_timeout(remaining: Duration, remaining_tiers: usize) -> Duration {
    let divisor = u128::try_from(remaining_tiers)
        .unwrap_or(u128::MAX)
        .saturating_add(1);
    let milliseconds = (remaining.as_millis() / divisor).max(1);
    Duration::from_millis(u64::try_from(milliseconds).unwrap_or(u64::MAX)).min(remaining)
}

pub(crate) fn deadline_exhausted(tier: &str) -> SearchResults {
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
                browser: HeadlessBrowser::Moli,
                browser_max_retries: 1,
            },
            CascadeControls::default(),
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("search timeout exceeds the platform limit"));
    }

    #[test]
    fn rejected_rows_are_dropped_before_the_structural_gate() {
        let mut results = SearchResults::new();
        for url in ["https://kept.example/a", "https://dropped.example/b"] {
            results.add_result(SearchResult::new(url, "title", "content"));
        }
        let admission: ResultAdmission = Arc::new(|result| result.url.contains("kept"));

        admit_results(&mut results, Some(&admission));

        assert_eq!(results.count, 1);
        assert_eq!(results.items()[0].url, "https://kept.example/a");
    }

    #[tokio::test]
    async fn host_provisioning_failure_is_a_typed_headless_failure() {
        use futures::FutureExt;

        let plan = EngineTierPlan::new(Some(&["g".to_string()]), None, None).unwrap();
        let pending = async { Err("host could not provision Moli".to_string()) }
            .boxed()
            .shared();
        let outcome = execute_cascade(
            &plan,
            CascadeRequest {
                query: SearchQuery::new("portable query"),
                limit: 1,
                timeout: Duration::from_secs(5),
                proxy: None,
                config: None,
                browser: HeadlessBrowser::Moli,
                browser_max_retries: 0,
            },
            CascadeControls {
                headless_runtime: Some(pending),
                ..CascadeControls::default()
            },
        )
        .await
        .unwrap();

        let failure = &outcome.results.failures()[0];
        assert_eq!(failure.engine, "moli");
        assert_eq!(failure.kind, "headless_backend_unavailable");
        assert!(failure.message.contains("host could not provision Moli"));
    }
}
