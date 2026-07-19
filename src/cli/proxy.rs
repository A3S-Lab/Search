//! CLI proxy scope and conventional HTTP fetcher construction.

use std::sync::Arc;

use anyhow::Result;

use a3s_search::{providers::BuiltinProvider, HttpFetcher, PageFetcher};

pub(crate) fn report_proxy_scope(
    proxy: Option<&str>,
    engine_shortcuts: &[String],
    text_output: bool,
) {
    if proxy.is_none() {
        return;
    }

    let uses_provider = engine_shortcuts
        .iter()
        .any(|shortcut| BuiltinProvider::from_id(shortcut).is_some());
    let uses_conventional = engine_shortcuts
        .iter()
        .any(|shortcut| BuiltinProvider::from_id(shortcut).is_none());

    if uses_provider && uses_conventional {
        eprintln!(
            "Proxy enabled for conventional engines; native provider API requests remain direct"
        );
    } else if uses_provider {
        eprintln!("The --proxy option does not apply; native provider API requests remain direct");
    } else if text_output {
        eprintln!("Proxy enabled for conventional engines");
    }
}

pub(crate) fn create_http_fetcher(
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
        Some(proxy) => HttpFetcher::with_proxy(proxy).map_err(|error| {
            anyhow::anyhow!("Failed to create HTTP fetcher with proxy: {error}")
        })?,
        None => HttpFetcher::new(),
    };
    Ok(Arc::new(fetcher))
}
