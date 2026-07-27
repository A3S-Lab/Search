//! Search-specific adapter for the typed A3S Browser renderer.
//!
//! Browser process ownership, provider installation, tab limits, rendering, and
//! cleanup live in `a3s-use-browser`. Search owns only URL-to-HTML adaptation,
//! wait-strategy mapping, retries, and search metrics.

use std::sync::Arc;
use std::time::{Duration, Instant};

use a3s_use_browser::{PageRenderer, RenderRequest, WaitCondition};
use async_trait::async_trait;
use tracing::warn;
use url::Url;

use crate::{Metrics, PageFetcher, Result, SearchError, WaitStrategy};

pub use a3s_use_browser::{BrowserBackend, BrowserPool, BrowserPoolConfig, BrowserProvider};

/// Adapts a typed Browser renderer to Search's HTML fetcher contract.
pub struct BrowserFetcher {
    renderer: Arc<dyn PageRenderer>,
    wait: WaitStrategy,
    user_agent: Option<String>,
    timeout: Duration,
    max_retries: u32,
    retry_delay_ms: u64,
    metrics: Option<Arc<Metrics>>,
}

impl BrowserFetcher {
    /// Creates an adapter from a concrete renderer such as `BrowserPool`.
    pub fn new<R>(renderer: Arc<R>) -> Self
    where
        R: PageRenderer + 'static,
    {
        Self::from_renderer(renderer)
    }

    /// Creates an adapter from an already type-erased renderer.
    pub fn from_renderer(renderer: Arc<dyn PageRenderer>) -> Self {
        Self {
            renderer,
            wait: WaitStrategy::default(),
            user_agent: None,
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay_ms: 100,
            metrics: None,
        }
    }

    /// Sets the Browser wait condition used for every fetch.
    pub fn with_wait(mut self, wait: WaitStrategy) -> Self {
        self.wait = wait;
        self
    }

    /// Overrides the user agent before navigation starts.
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Sets the maximum time for one render attempt.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets retries after the initial attempt and the exponential-backoff base.
    pub fn with_retries(mut self, max_retries: u32, retry_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.retry_delay_ms = retry_delay_ms;
        self
    }

    /// Attaches Search's fetch metrics registry.
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    async fn fetch_with_retry(&self, url: &str) -> Result<String> {
        let mut attempt = 0;
        let mut delay = Duration::from_millis(self.retry_delay_ms);

        loop {
            match self.render_once(url).await {
                Ok(html) => return Ok(html),
                Err(error) if attempt < self.max_retries && error.is_transient() => {
                    attempt += 1;
                    warn!(
                        "Transient Browser error on {url}, retry {attempt}/{} after {delay:?}: {error}",
                        self.max_retries
                    );
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn render_once(&self, value: &str) -> Result<String> {
        let request = RenderRequest {
            url: Url::parse(value)?,
            timeout_ms: duration_millis(self.timeout),
            wait: map_wait_strategy(&self.wait),
            user_agent: self.user_agent.clone(),
            screenshot_path: None,
        };
        self.renderer
            .render(request)
            .await
            .map(|page| page.html)
            .map_err(map_use_error)
    }
}

#[async_trait]
impl PageFetcher for BrowserFetcher {
    async fn fetch(&self, url: &str) -> Result<String> {
        let started = Instant::now();
        let result = self.fetch_with_retry(url).await;
        if let Some(metrics) = self.metrics.as_ref() {
            match &result {
                Ok(_) => metrics.record_success(started.elapsed()),
                Err(error) => metrics.record_failure(error.kind(), error.is_transient()),
            }
        }
        result
    }
}

fn map_wait_strategy(wait: &WaitStrategy) -> WaitCondition {
    match wait {
        WaitStrategy::Load => WaitCondition::Load,
        WaitStrategy::NetworkIdle { idle_ms } => WaitCondition::NetworkIdle { idle_ms: *idle_ms },
        WaitStrategy::Selector { css, timeout_ms } => WaitCondition::Selector {
            css: css.clone(),
            timeout_ms: *timeout_ms,
        },
        WaitStrategy::Delay { ms } => WaitCondition::Delay { ms: *ms },
    }
}

fn map_use_error(error: a3s_use_browser::UseError) -> SearchError {
    let mut message = format!("{}: {}", error.code, error.message);
    if let Some(suggestion) = error.suggestion {
        message.push_str(" Suggestion: ");
        message.push_str(&suggestion);
    }
    SearchError::Browser(message)
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use a3s_use_browser::{RenderedPage, UseError, UseResult};

    use super::*;

    struct RecordingRenderer {
        requests: Mutex<Vec<RenderRequest>>,
        failures_remaining: AtomicUsize,
    }

    impl RecordingRenderer {
        fn new(failures: usize) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                failures_remaining: AtomicUsize::new(failures),
            }
        }
    }

    #[async_trait]
    impl PageRenderer for RecordingRenderer {
        async fn render(&self, request: RenderRequest) -> UseResult<RenderedPage> {
            self.requests.lock().unwrap().push(request.clone());
            if self
                .failures_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(UseError::new("use.browser.timeout", "fixture timeout"));
            }
            Ok(RenderedPage {
                requested_url: request.url.clone(),
                final_url: request.url,
                status: Some(200),
                content_type: Some("text/html".to_string()),
                html: "<main>rendered</main>".to_string(),
                elapsed_ms: 1,
                artifacts: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn injected_renderer_receives_typed_search_options() {
        let renderer = Arc::new(RecordingRenderer::new(0));
        let metrics = Arc::new(Metrics::new());
        let fetcher = BrowserFetcher::new(Arc::clone(&renderer))
            .with_wait(WaitStrategy::Selector {
                css: "main".to_string(),
                timeout_ms: 1_500,
            })
            .with_user_agent("a3s-search-test")
            .with_timeout(Duration::from_secs(5))
            .with_metrics(Arc::clone(&metrics));

        let html = fetcher.fetch("https://example.com/page").await.unwrap();

        assert_eq!(html, "<main>rendered</main>");
        let requests = renderer.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].timeout_ms, 5_000);
        assert_eq!(requests[0].user_agent.as_deref(), Some("a3s-search-test"));
        assert!(matches!(
            &requests[0].wait,
            WaitCondition::Selector { css, timeout_ms }
                if css == "main" && *timeout_ms == 1_500
        ));
        assert_eq!(metrics.total_requests(), 1);
    }

    #[tokio::test]
    async fn transient_use_errors_are_retried_by_search() {
        let renderer = Arc::new(RecordingRenderer::new(2));
        let fetcher = BrowserFetcher::new(Arc::clone(&renderer)).with_retries(2, 0);

        let html = fetcher.fetch("https://example.com").await.unwrap();

        assert!(html.contains("rendered"));
        assert_eq!(renderer.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn use_error_code_is_preserved_in_search_error_context() {
        let renderer = Arc::new(RecordingRenderer::new(1));
        let error = BrowserFetcher::new(renderer)
            .with_retries(0, 0)
            .fetch("https://example.com")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("use.browser.timeout"));
    }
}
