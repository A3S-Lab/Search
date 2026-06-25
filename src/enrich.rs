//! Optional enrichment pass: fetch result pages and fill in extracted article text.
//!
//! Search engines only return snippets. This pass fetches each result's page through
//! the same [`PageFetcher`] used for SERPs — so it reuses the proxy pool, headless
//! browser, and health machinery for free — and stores the extracted main content in
//! [`SearchResult::full_text`](crate::SearchResult::full_text).

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use tokio::time::timeout;
use tracing::debug;

use crate::{extract::extract_main_text, PageFetcher, SearchResults};

/// Fetches each result's page through `fetcher`, extracts the main article text, and
/// stores it in [`SearchResult::full_text`](crate::SearchResult::full_text).
///
/// A successful extraction overwrites any prior `full_text`; a fetch error, timeout, or
/// unextractable page is skipped and leaves the existing value (and the snippet in
/// `content`) untouched — so this pass only ever *adds* information.
///
/// - `concurrency` bounds in-flight fetches.
/// - `per_page` is the per-URL fetch timeout.
///
/// Pass an `HttpFetcher`/`PooledHttpFetcher` for static pages, or a `BrowserFetcher`
/// (requires the `headless` feature) for JavaScript-rendered pages.
pub async fn enrich_full_text(
    results: &mut SearchResults,
    fetcher: Arc<dyn PageFetcher>,
    concurrency: usize,
    per_page: Duration,
) {
    let urls: Vec<(usize, String)> = results
        .items()
        .iter()
        .enumerate()
        .map(|(i, r)| (i, r.url.clone()))
        .collect();

    let texts: Vec<(usize, Option<String>)> = stream::iter(urls.into_iter().map(|(i, url)| {
        let fetcher = Arc::clone(&fetcher);
        async move {
            let html = match timeout(per_page, fetcher.fetch(&url)).await {
                Ok(Ok(html)) => html,
                Ok(Err(e)) => {
                    debug!(%url, error = %e, "reader fetch failed");
                    return (i, None);
                }
                Err(_) => {
                    debug!(%url, "reader fetch timed out");
                    return (i, None);
                }
            };
            // ponytail: Readability is CPU-bound; keep it off the async workers.
            let url_for_log = url.clone();
            let text =
                match tokio::task::spawn_blocking(move || extract_main_text(&html, &url)).await {
                    Ok(text) => text,
                    Err(e) => {
                        // A panic inside Readability degrades to a miss, but log it so it is
                        // distinguishable from a page that simply had no article body.
                        debug!(url = %url_for_log, error = %e, "reader extraction panicked");
                        None
                    }
                };
            (i, text)
        }
    }))
    .buffer_unordered(concurrency)
    .collect()
    .await;

    let items = results.items_mut();
    for (i, text) in texts {
        if let Some(t) = text {
            items[i].full_text = Some(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SearchError, SearchResult};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const ARTICLE: &str = r#"<html><head><title>T</title></head><body>
        <nav>home about UNIQUE_NAV login signup</nav>
        <article>
          <h1>Headline</h1>
          <p>BODY_MARKER This is the principal article body that Readability should
          keep. It contains several full sentences so the algorithm scores it as the
          main content rather than discarding it as page noise.</p>
          <p>A second substantial paragraph adds more textual weight to the article
          node, ensuring the extractor selects this block over the surrounding nav,
          sidebar, and footer chrome present on the page.</p>
          <p>A third paragraph keeps the text density high and the link ratio low,
          which is what the scoring heuristic rewards when choosing the dominant
          content container of an HTML document.</p>
        </article>
        <footer>FOOTER_MARKER copyright 2026</footer>
    </body></html>"#;

    /// Canned response for a URL.
    enum Resp {
        Html(&'static str),
        Fail,
        Slow(Duration, &'static str),
    }

    /// In-memory `PageFetcher`: no sockets, fully deterministic.
    struct MockFetcher {
        map: HashMap<String, Resp>,
    }

    #[async_trait]
    impl PageFetcher for MockFetcher {
        async fn fetch(&self, url: &str) -> crate::Result<String> {
            match self.map.get(url) {
                Some(Resp::Html(h)) => Ok(h.to_string()),
                Some(Resp::Slow(d, h)) => {
                    tokio::time::sleep(*d).await;
                    Ok(h.to_string())
                }
                Some(Resp::Fail) | None => Err(SearchError::Network("mock failure".into())),
            }
        }
    }

    fn fetcher(pairs: Vec<(&str, Resp)>) -> Arc<MockFetcher> {
        let map = pairs.into_iter().map(|(u, r)| (u.to_string(), r)).collect();
        Arc::new(MockFetcher { map })
    }

    fn results_for(urls: &[&str]) -> SearchResults {
        let mut r = SearchResults::new();
        for (i, u) in urls.iter().enumerate() {
            r.add_result(SearchResult::new(
                *u,
                format!("title{i}"),
                format!("snippet{i}"),
            ));
        }
        r
    }

    #[tokio::test]
    async fn fills_full_text_on_success() {
        let mut results = results_for(&["u0"]);
        enrich_full_text(
            &mut results,
            fetcher(vec![("u0", Resp::Html(ARTICLE))]),
            4,
            Duration::from_secs(5),
        )
        .await;

        let item = &results.items()[0];
        let body = item.full_text.as_deref().expect("full_text populated");
        assert!(body.contains("BODY_MARKER"), "kept the article body");
        assert!(!body.contains("FOOTER_MARKER"), "dropped the footer");
        assert_eq!(item.content, "snippet0", "snippet preserved");
    }

    #[tokio::test]
    async fn skips_on_fetch_error_keeps_snippet() {
        let mut results = results_for(&["u0"]);
        enrich_full_text(
            &mut results,
            fetcher(vec![("u0", Resp::Fail)]),
            4,
            Duration::from_secs(5),
        )
        .await;

        assert!(results.items()[0].full_text.is_none());
        assert_eq!(results.items()[0].content, "snippet0");
    }

    #[tokio::test]
    async fn skips_on_timeout() {
        let mut results = results_for(&["u0"]);
        // Mock sleeps far longer than the per-page timeout -> extraction never runs.
        enrich_full_text(
            &mut results,
            fetcher(vec![("u0", Resp::Slow(Duration::from_secs(30), ARTICLE))]),
            4,
            Duration::from_millis(100),
        )
        .await;

        assert!(results.items()[0].full_text.is_none());
    }

    #[tokio::test]
    async fn mixed_batch_maps_indices_correctly() {
        // buffer_unordered completes out of order; the write-back must stay index-keyed.
        let mut results = results_for(&["fail", "ok", "missing"]);
        enrich_full_text(
            &mut results,
            fetcher(vec![("ok", Resp::Html(ARTICLE)), ("fail", Resp::Fail)]),
            8,
            Duration::from_secs(5),
        )
        .await;

        assert!(results.items()[0].full_text.is_none(), "fail -> None");
        assert!(
            results.items()[1]
                .full_text
                .as_deref()
                .unwrap_or("")
                .contains("BODY_MARKER"),
            "ok -> body"
        );
        assert!(results.items()[2].full_text.is_none(), "missing -> None");
        assert_eq!(results.items()[1].content, "snippet1", "snippet preserved");
    }

    #[tokio::test]
    async fn empty_body_page_leaves_none() {
        let mut results = results_for(&["u0"]);
        enrich_full_text(
            &mut results,
            fetcher(vec![(
                "u0",
                Resp::Html("<html><head><title>t</title></head><body></body></html>"),
            )]),
            4,
            Duration::from_secs(5),
        )
        .await;

        assert!(results.items()[0].full_text.is_none());
    }

    #[tokio::test]
    async fn empty_results_no_panic() {
        let mut results = SearchResults::new();
        enrich_full_text(&mut results, fetcher(vec![]), 4, Duration::from_secs(1)).await;
        assert_eq!(results.items().len(), 0);
    }

    #[tokio::test]
    async fn concurrency_one_does_not_deadlock() {
        let mut results = results_for(&["a", "b", "c"]);
        enrich_full_text(
            &mut results,
            fetcher(vec![
                ("a", Resp::Html(ARTICLE)),
                ("b", Resp::Html(ARTICLE)),
                ("c", Resp::Html(ARTICLE)),
            ]),
            1, // serialize all fetches through a single slot
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(results.items().len(), 3);
        for item in results.items() {
            assert!(item
                .full_text
                .as_deref()
                .unwrap_or("")
                .contains("BODY_MARKER"));
        }
    }

    /// Fetcher that records the peak number of concurrently in-flight fetches.
    struct GaugeFetcher {
        inflight: AtomicUsize,
        peak: AtomicUsize,
    }

    #[async_trait]
    impl PageFetcher for GaugeFetcher {
        async fn fetch(&self, _url: &str) -> crate::Result<String> {
            let now = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            Ok(ARTICLE.to_string())
        }
    }

    #[tokio::test]
    async fn concurrency_caps_in_flight_fetches() {
        // The documented contract is an *upper* bound: never more than `concurrency`
        // fetches in flight. An unbounded impl (join_all) would peak at 10 and fail this.
        let gauge = Arc::new(GaugeFetcher {
            inflight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        });
        let urls: Vec<String> = (0..10)
            .map(|i| format!("https://example.com/{i}"))
            .collect();
        let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
        let mut results = results_for(&refs);

        enrich_full_text(
            &mut results,
            Arc::clone(&gauge) as Arc<dyn PageFetcher>,
            3,
            Duration::from_secs(5),
        )
        .await;

        let peak = gauge.peak.load(Ordering::SeqCst);
        assert!(
            peak <= 3,
            "peak in-flight {peak} exceeded the concurrency cap of 3"
        );
        assert_eq!(
            results
                .items()
                .iter()
                .filter(|r| r.full_text.is_some())
                .count(),
            10,
            "all results still enriched"
        );
    }

    #[tokio::test]
    async fn success_overwrites_existing_full_text() {
        let mut results = results_for(&["u0"]);
        results.items_mut()[0].full_text = Some("OLD".to_string());
        enrich_full_text(
            &mut results,
            fetcher(vec![("u0", Resp::Html(ARTICLE))]),
            4,
            Duration::from_secs(5),
        )
        .await;

        let ft = results.items()[0].full_text.as_deref().unwrap();
        assert!(ft.contains("BODY_MARKER"));
        assert_ne!(ft, "OLD", "success replaces the prior value");
    }

    #[tokio::test]
    async fn failure_preserves_existing_full_text() {
        let mut results = results_for(&["u0"]);
        results.items_mut()[0].full_text = Some("OLD".to_string());
        enrich_full_text(
            &mut results,
            fetcher(vec![("u0", Resp::Fail)]),
            4,
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(
            results.items()[0].full_text.as_deref(),
            Some("OLD"),
            "a failed fetch must not clear an existing value"
        );
    }
}
