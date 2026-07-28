//! A3S Search CLI - Meta search engine command line interface.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use a3s_search::{
    engines::{
        Bing, BingChina, BingParser, Brave, BraveParser, DuckDuckGo, DuckDuckGoParser, So360,
        So360Parser, Sogou, SogouParser, Wikipedia,
    },
    providers::BuiltinProvider,
    Engine, EngineConfig, HttpFetcher, SafeSearch, Search, SearchConfig, SearchQuery, TimeRange,
};

#[cfg(feature = "headless")]
use a3s_search::{
    a3s_use_browser::PageRenderer,
    browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig},
    engines::{Baidu, Google},
    PageFetcher, WaitStrategy,
};

mod cli;

use cli::output::{print_results, OutputFormat};
use cli::provider::{
    create_provider_engine, ensure_provider_ready, list_engines, load_search_config,
};
use cli::proxy::{create_http_fetcher, report_proxy_scope};

#[cfg(test)]
use a3s_search::{
    providers::{ProviderAuthentication, ProviderReadiness},
    SearchResults,
};
#[cfg(test)]
use cli::output::{json_output, truncate_str};
#[cfg(test)]
use cli::provider::provider_readiness_summary;

/// Extensible web search CLI with native providers and meta-search engines
#[derive(Parser)]
#[command(name = "a3s-search")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Search query (if no subcommand is provided)
    query: Option<String>,

    /// Search engines to use (comma-separated)
    /// Available: ddg, brave, bing, wiki, sogou, 360, g, baidu, bing_cn, anysearch, tavily
    #[arg(short, long, value_delimiter = ',')]
    engines: Option<Vec<String>>,

    /// Maximum number of results to display
    #[arg(short, long, default_value = "10")]
    limit: usize,

    /// Search timeout in seconds (overrides config/default)
    #[arg(short, long)]
    timeout: Option<u64>,

    /// Result page to fetch (1-indexed)
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    page: u32,

    /// Search language/locale, such as en, en-US, or zh
    #[arg(long)]
    language: Option<String>,

    /// Safe search level
    #[arg(long)]
    safesearch: Option<SafeSearchArg>,

    /// Time range filter
    #[arg(long)]
    time_range: Option<TimeRangeArg>,

    /// Output format
    #[arg(short, long, default_value = "text")]
    format: OutputFormat,

    /// Proxy URL (e.g., http://127.0.0.1:8080 or socks5://127.0.0.1:1080)
    #[arg(short, long)]
    proxy: Option<String>,

    /// ACL configuration file
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Use headless browser for JS-rendered engines (default: auto-detected)
    #[arg(long, hide = true)]
    headless: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List available search engines
    Engines,
    /// Update a3s-search to the latest version
    Update,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum SafeSearchArg {
    Off,
    Moderate,
    Strict,
}

impl From<SafeSearchArg> for SafeSearch {
    fn from(value: SafeSearchArg) -> Self {
        match value {
            SafeSearchArg::Off => SafeSearch::Off,
            SafeSearchArg::Moderate => SafeSearch::Moderate,
            SafeSearchArg::Strict => SafeSearch::Strict,
        }
    }
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum TimeRangeArg {
    Day,
    Week,
    Month,
    Year,
}

impl From<TimeRangeArg> for TimeRange {
    fn from(value: TimeRangeArg) -> Self {
        match value {
            TimeRangeArg::Day => TimeRange::Day,
            TimeRangeArg::Week => TimeRange::Week,
            TimeRangeArg::Month => TimeRange::Month,
            TimeRangeArg::Year => TimeRange::Year,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    if cli.verbose {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::DEBUG)
            .finish();
        tracing::subscriber::set_global_default(subscriber)?;
    }

    match cli.command {
        Some(Commands::Engines) => list_engines(cli.config.as_deref()),
        Some(Commands::Update) => {
            a3s_updater::run_update(&a3s_updater::UpdateConfig {
                binary_name: "a3s-search",
                crate_name: "a3s-search",
                current_version: env!("CARGO_PKG_VERSION"),
                github_owner: "A3S-Lab",
                github_repo: "Search",
            })
            .await
        }
        None => {
            if let Some(query) = cli.query {
                run_search(SearchArgs {
                    query,
                    engines: cli.engines,
                    limit: cli.limit,
                    timeout: cli.timeout,
                    page: cli.page,
                    language: cli.language,
                    safesearch: cli.safesearch,
                    time_range: cli.time_range,
                    format: cli.format,
                    proxy: cli.proxy,
                    config: cli.config,
                })
                .await
            } else {
                // No query provided, show help
                println!("A3S Search - Extensible web search CLI\n");
                println!("Usage: a3s-search <QUERY> [OPTIONS]");
                println!("       a3s-search engines\n");
                println!("Examples:");
                println!("  a3s-search \"Rust programming\"");
                println!("  a3s-search \"Rust\" -e ddg,wiki -l 5");
                println!("  a3s-search \"Rust\" -f json");
                println!("  a3s-search \"Rust\" -p http://127.0.0.1:8080\n");
                println!("Options:");
                println!(
                    "  -e, --engines <ENGINES>  Engines/providers: ddg,brave,bing,wiki,sogou,360,g,baidu,bing_cn,anysearch,tavily"
                );
                println!("  -l, --limit <N>          Max results (default: 10)");
                println!("  -t, --timeout <SECS>     Timeout in seconds");
                println!("      --page <N>           Result page (default: 1)");
                println!("      --language <LOCALE>  Search language/locale");
                println!("      --safesearch <LEVEL> off, moderate, strict");
                println!("      --time-range <RANGE> day, week, month, year");
                println!("  -f, --format <FORMAT>    Output: text, json, compact");
                println!("  -p, --proxy <URL>        Proxy URL (http/https/socks5)");
                println!("  -c, --config <PATH>      ACL configuration file");
                println!("  -v, --verbose            Enable debug logging");
                println!("  -h, --help               Show help");
                println!("  -V, --version            Show version\n");
                println!("Run 'a3s-search engines' to list all available engines.");
                Ok(())
            }
        }
    }
}

struct SearchArgs {
    query: String,
    engines: Option<Vec<String>>,
    limit: usize,
    timeout: Option<u64>,
    page: u32,
    language: Option<String>,
    safesearch: Option<SafeSearchArg>,
    time_range: Option<TimeRangeArg>,
    format: OutputFormat,
    proxy: Option<String>,
    config: Option<PathBuf>,
}

fn selected_engine_shortcuts(args: &SearchArgs, config: Option<&SearchConfig>) -> Vec<String> {
    if let Some(engines) = &args.engines {
        return engines.clone();
    }

    if let Some(config) = config {
        if !config.engines.is_empty() || !config.providers.is_empty() {
            return config
                .enabled_sources()
                .into_iter()
                .map(str::to_string)
                .collect();
        }
    }

    vec!["ddg".to_string(), "wiki".to_string()]
}

fn is_config_enabled(config: Option<&SearchConfig>, shortcut: &str) -> bool {
    config
        .and_then(|config| {
            config
                .engine_entry(shortcut)
                .map(|entry| entry.enabled)
                .or_else(|| config.provider_entry(shortcut).map(|entry| entry.enabled))
        })
        .unwrap_or(true)
}

fn configured_engine_config(
    config: Option<&SearchConfig>,
    engine_config: EngineConfig,
) -> EngineConfig {
    if let Some(config) = config {
        config.apply_engine_config(engine_config)
    } else {
        engine_config
    }
}

async fn run_search(args: SearchArgs) -> Result<()> {
    let config = load_search_config(args.config.as_deref())?;

    let mut search = if let Some(config) = config.as_ref() {
        Search::with_health_config(config.health_config())
    } else {
        Search::new()
    };

    if let Some(timeout) = args.timeout {
        search.set_timeout(Duration::from_secs(timeout));
    } else if config.is_none() {
        search.set_timeout(Duration::from_secs(10));
    }

    let engine_shortcuts = selected_engine_shortcuts(&args, config.as_ref());
    report_proxy_scope(
        args.proxy.as_deref(),
        &engine_shortcuts,
        matches!(args.format, OutputFormat::Text),
    );

    // Warn if headless engines are requested without the feature
    #[cfg(not(feature = "headless"))]
    {
        let headless_engines = ["g", "google", "baidu"];
        for e in &engine_shortcuts {
            if headless_engines.contains(&e.as_str()) {
                eprintln!(
                    "Warning: '{}' engine requires the 'headless' feature. \
                     Rebuild with: cargo build --features headless",
                    e
                );
            }
        }
    }

    // Lazily create browser pool when headless engines are needed
    #[cfg(feature = "headless")]
    let browser_pool: std::sync::Arc<BrowserPool> = {
        let pool_config = BrowserPoolConfig {
            proxy_url: args.proxy.clone(),
            ..Default::default()
        };
        std::sync::Arc::new(BrowserPool::new(pool_config))
    };
    #[cfg(feature = "headless")]
    let browser_renderer: std::sync::Arc<dyn PageRenderer> = browser_pool.clone();

    let http_fetcher = create_http_fetcher(args.proxy.as_deref(), &engine_shortcuts)?;

    for shortcut in &engine_shortcuts {
        if !is_config_enabled(config.as_ref(), shortcut) {
            if args.engines.is_some() {
                eprintln!(
                    "Warning: '{}' engine is disabled by config, skipping",
                    shortcut
                );
            }
            continue;
        }

        if let Some(provider) = BuiltinProvider::from_id(shortcut) {
            let engine = create_provider_engine(provider, config.as_ref())?;
            ensure_provider_ready(&engine)?;
            search.add_engine(engine);
            continue;
        }

        match shortcut.as_str() {
            "ddg" | "duckduckgo" => {
                let engine = DuckDuckGo::with_fetcher(
                    DuckDuckGoParser,
                    std::sync::Arc::clone(&http_fetcher),
                );
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "brave" => {
                let engine = Brave::with_fetcher(BraveParser, std::sync::Arc::clone(&http_fetcher));
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "bing" => {
                let engine = Bing::with_fetcher(BingParser, std::sync::Arc::clone(&http_fetcher));
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "wiki" | "wikipedia" => {
                // Wikipedia needs its own fetcher since it uses JSON API, not HTML
                let fetcher = if let Some(proxy_url) = &args.proxy {
                    HttpFetcher::with_proxy(proxy_url).map_err(|e| {
                        anyhow::anyhow!("Failed to create HTTP fetcher with proxy: {}", e)
                    })?
                } else {
                    HttpFetcher::new()
                };
                let engine = Wikipedia::with_http_fetcher(fetcher);
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "sogou" => {
                let engine = Sogou::with_fetcher(SogouParser, std::sync::Arc::clone(&http_fetcher));
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "360" | "so360" => {
                let engine = So360::with_fetcher(So360Parser, std::sync::Arc::clone(&http_fetcher));
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "bing_cn" => {
                let engine = BingChina::new(std::sync::Arc::clone(&http_fetcher));
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            #[cfg(feature = "headless")]
            "g" | "google" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> = std::sync::Arc::new(
                    BrowserFetcher::from_renderer(std::sync::Arc::clone(&browser_renderer))
                        .with_wait(WaitStrategy::Selector {
                            css: "div.g".to_string(),
                            timeout_ms: 5000,
                        }),
                );
                let engine = Google::new(fetcher);
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            #[cfg(feature = "headless")]
            "baidu" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> = std::sync::Arc::new(
                    BrowserFetcher::from_renderer(std::sync::Arc::clone(&browser_renderer))
                        .with_wait(WaitStrategy::Selector {
                            css: "div.c-container".to_string(),
                            timeout_ms: 5000,
                        }),
                );
                let engine = Baidu::new(fetcher);
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            #[cfg(not(feature = "headless"))]
            "g" | "google" | "baidu" => {
                eprintln!(
                    "Warning: '{}' engine requires the 'headless' feature. \
                     Rebuild with: cargo build --features headless",
                    shortcut
                );
            }
            _ => {
                eprintln!("Warning: Unknown engine '{}', skipping", shortcut);
            }
        }
    }

    if search.engine_count() == 0 {
        anyhow::bail!("No valid engines specified");
    }

    // Perform search
    let mut query = SearchQuery::new(&args.query).with_page(args.page);
    if let Some(language) = &args.language {
        query = query.with_language(language);
    }
    if let Some(safesearch) = args.safesearch {
        query = query.with_safesearch(safesearch.into());
    }
    if let Some(time_range) = args.time_range {
        query = query.with_time_range(time_range.into());
    }
    let search_result = search.search(query).await;
    #[cfg(feature = "headless")]
    browser_pool.shutdown().await;
    let results = search_result?;

    // Show engine errors to the user
    for (engine, error) in results.errors() {
        eprintln!("Warning: {} engine failed: {}", engine, error);
    }

    print_results(&args.query, &results, args.limit, args.format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_search::proxy::{ProxyConfig, ProxyProtocol};
    use clap::CommandFactory;

    fn parse_proxy_url(url: &str) -> Result<ProxyConfig> {
        let url = url::Url::parse(url)?;

        let protocol = match url.scheme() {
            "http" => ProxyProtocol::Http,
            "https" => ProxyProtocol::Https,
            "socks5" => ProxyProtocol::Socks5,
            scheme => anyhow::bail!("Unsupported proxy protocol: {}", scheme),
        };

        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("Missing proxy host"))?;
        let port = url.port().unwrap_or(match protocol {
            ProxyProtocol::Http => 8080,
            ProxyProtocol::Https => 443,
            ProxyProtocol::Socks5 => 1080,
        });

        let mut config = ProxyConfig::new(host, port).with_protocol(protocol);

        if let Some(password) = url.password() {
            config = config.with_auth(url.username(), password);
        }

        Ok(config)
    }

    #[test]
    fn test_cli_parse_help() {
        // Verify CLI structure is valid
        Cli::command().debug_assert();
    }

    #[test]
    fn test_parse_proxy_url_http() {
        let config = parse_proxy_url("http://127.0.0.1:8080").unwrap();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert_eq!(config.protocol, ProxyProtocol::Http);
        assert!(config.username.is_none());
        assert!(config.password.is_none());
    }

    #[test]
    fn test_parse_proxy_url_https() {
        let config = parse_proxy_url("https://proxy.example.com:443").unwrap();
        assert_eq!(config.host, "proxy.example.com");
        assert_eq!(config.port, 443);
        assert_eq!(config.protocol, ProxyProtocol::Https);
    }

    #[test]
    fn test_parse_proxy_url_socks5() {
        let config = parse_proxy_url("socks5://localhost:1080").unwrap();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 1080);
        assert_eq!(config.protocol, ProxyProtocol::Socks5);
    }

    #[test]
    fn test_parse_proxy_url_with_auth() {
        let config = parse_proxy_url("http://user:pass@127.0.0.1:8080").unwrap();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
    }

    #[test]
    fn test_parse_proxy_url_default_http_port() {
        let config = parse_proxy_url("http://127.0.0.1").unwrap();
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_parse_proxy_url_default_socks5_port() {
        let config = parse_proxy_url("socks5://127.0.0.1").unwrap();
        assert_eq!(config.port, 1080);
    }

    #[test]
    fn test_parse_proxy_url_unsupported_protocol() {
        let result = parse_proxy_url("ftp://127.0.0.1:21");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported proxy protocol"));
    }

    #[test]
    fn test_parse_proxy_url_invalid_url() {
        let result = parse_proxy_url("not-a-valid-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_output_format_values() {
        // Test that all output formats can be created
        let _text = OutputFormat::Text;
        let _json = OutputFormat::Json;
        let _compact = OutputFormat::Compact;
    }

    #[test]
    fn test_cli_with_query() {
        let cli = Cli::parse_from(["a3s-search", "test query"]);
        assert_eq!(cli.query, Some("test query".to_string()));
        assert!(cli.engines.is_none());
        assert_eq!(cli.limit, 10);
        assert_eq!(cli.timeout, None);
        assert_eq!(cli.page, 1);
        assert!(cli.language.is_none());
        assert!(cli.safesearch.is_none());
        assert!(cli.time_range.is_none());
        assert!(cli.proxy.is_none());
        assert!(cli.config.is_none());
        assert!(!cli.verbose);
    }

    #[test]
    fn test_cli_with_engines() {
        let cli = Cli::parse_from(["a3s-search", "query", "-e", "ddg,anysearch,tavily"]);
        assert_eq!(
            cli.engines,
            Some(vec![
                "ddg".to_string(),
                "anysearch".to_string(),
                "tavily".to_string()
            ])
        );
    }

    #[test]
    fn test_cli_with_limit() {
        let cli = Cli::parse_from(["a3s-search", "query", "-l", "5"]);
        assert_eq!(cli.limit, 5);
    }

    #[test]
    fn test_cli_with_timeout() {
        let cli = Cli::parse_from(["a3s-search", "query", "-t", "30"]);
        assert_eq!(cli.timeout, Some(30));
    }

    #[test]
    fn test_cli_with_config() {
        let cli = Cli::parse_from(["a3s-search", "query", "-c", "search.acl"]);
        assert_eq!(cli.config, Some(PathBuf::from("search.acl")));
    }

    #[test]
    fn test_cli_with_query_filters() {
        let cli = Cli::parse_from([
            "a3s-search",
            "query",
            "--page",
            "3",
            "--language",
            "zh-CN",
            "--safesearch",
            "strict",
            "--time-range",
            "week",
        ]);

        assert_eq!(cli.page, 3);
        assert_eq!(cli.language, Some("zh-CN".to_string()));
        assert!(matches!(cli.safesearch, Some(SafeSearchArg::Strict)));
        assert!(matches!(cli.time_range, Some(TimeRangeArg::Week)));
    }

    #[test]
    fn test_cli_with_format_json() {
        let cli = Cli::parse_from(["a3s-search", "query", "-f", "json"]);
        assert!(matches!(cli.format, OutputFormat::Json));
    }

    #[test]
    fn test_cli_with_format_compact() {
        let cli = Cli::parse_from(["a3s-search", "query", "-f", "compact"]);
        assert!(matches!(cli.format, OutputFormat::Compact));
    }

    #[test]
    fn test_cli_with_proxy() {
        let cli = Cli::parse_from(["a3s-search", "query", "-p", "http://127.0.0.1:8080"]);
        assert_eq!(cli.proxy, Some("http://127.0.0.1:8080".to_string()));
    }

    #[test]
    fn test_cli_with_verbose() {
        let cli = Cli::parse_from(["a3s-search", "query", "-v"]);
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_all_options() {
        let cli = Cli::parse_from([
            "a3s-search",
            "rust programming",
            "-e",
            "ddg,wiki,sogou",
            "-l",
            "20",
            "-t",
            "15",
            "-f",
            "json",
            "-p",
            "socks5://localhost:1080",
            "-c",
            "search.acl",
            "--page",
            "2",
            "--language",
            "en-US",
            "--safesearch",
            "moderate",
            "--time-range",
            "month",
            "-v",
        ]);
        assert_eq!(cli.query, Some("rust programming".to_string()));
        assert_eq!(
            cli.engines,
            Some(vec![
                "ddg".to_string(),
                "wiki".to_string(),
                "sogou".to_string()
            ])
        );
        assert_eq!(cli.limit, 20);
        assert_eq!(cli.timeout, Some(15));
        assert_eq!(cli.page, 2);
        assert_eq!(cli.language, Some("en-US".to_string()));
        assert!(matches!(cli.safesearch, Some(SafeSearchArg::Moderate)));
        assert!(matches!(cli.time_range, Some(TimeRangeArg::Month)));
        assert!(matches!(cli.format, OutputFormat::Json));
        assert_eq!(cli.proxy, Some("socks5://localhost:1080".to_string()));
        assert_eq!(cli.config, Some(PathBuf::from("search.acl")));
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_engines_subcommand() {
        let cli = Cli::parse_from(["a3s-search", "engines"]);
        assert!(matches!(cli.command, Some(Commands::Engines)));
    }

    #[test]
    fn test_cli_engines_subcommand_accepts_config() {
        let cli = Cli::parse_from(["a3s-search", "--config", "search.acl", "engines"]);
        assert!(matches!(cli.command, Some(Commands::Engines)));
        assert_eq!(cli.config, Some(PathBuf::from("search.acl")));
    }

    #[test]
    fn test_cli_no_args() {
        let cli = Cli::parse_from(["a3s-search"]);
        assert!(cli.query.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_with_headless() {
        let cli = Cli::parse_from(["a3s-search", "query", "--headless"]);
        assert!(cli.headless);
    }

    #[test]
    fn test_cli_headless_default_false() {
        let cli = Cli::parse_from(["a3s-search", "query"]);
        assert!(!cli.headless);
    }

    #[test]
    fn test_cli_headless_with_google_engine() {
        let cli = Cli::parse_from(["a3s-search", "query", "-e", "g,ddg", "--headless"]);
        assert!(cli.headless);
        assert_eq!(cli.engines, Some(vec!["g".to_string(), "ddg".to_string()]));
    }

    #[test]
    fn test_selected_engine_shortcuts_default() {
        let args = SearchArgs {
            query: "query".to_string(),
            engines: None,
            limit: 10,
            timeout: None,
            page: 1,
            language: None,
            safesearch: None,
            time_range: None,
            format: OutputFormat::Text,
            proxy: None,
            config: None,
        };

        assert_eq!(selected_engine_shortcuts(&args, None), vec!["ddg", "wiki"]);
    }

    #[test]
    fn test_selected_engine_shortcuts_from_config() {
        let args = SearchArgs {
            query: "query".to_string(),
            engines: None,
            limit: 10,
            timeout: None,
            page: 1,
            language: None,
            safesearch: None,
            time_range: None,
            format: OutputFormat::Text,
            proxy: None,
            config: None,
        };
        let config = SearchConfig::parse(
            r#"
            engine "ddg" { enabled = true }
            engine "brave" { enabled = false }
            engine "wiki" { enabled = true }
            provider "anysearch" { enabled = true }
            provider "tavily" { enabled = false }
            "#,
        )
        .unwrap();

        let selected = selected_engine_shortcuts(&args, Some(&config));
        assert_eq!(selected, vec!["anysearch", "ddg", "wiki"]);
        assert!(selected.contains(&"ddg".to_string()));
        assert!(selected.contains(&"wiki".to_string()));
        assert!(selected.contains(&"anysearch".to_string()));
        assert!(!selected.contains(&"brave".to_string()));
        assert!(!selected.contains(&"tavily".to_string()));
    }

    #[test]
    fn test_selected_engine_shortcuts_cli_overrides_config_selection() {
        let args = SearchArgs {
            query: "query".to_string(),
            engines: Some(vec!["bing".to_string()]),
            limit: 10,
            timeout: None,
            page: 1,
            language: None,
            safesearch: None,
            time_range: None,
            format: OutputFormat::Text,
            proxy: None,
            config: None,
        };
        let config = SearchConfig::parse(r#"engine "ddg" { enabled = true }"#).unwrap();

        assert_eq!(
            selected_engine_shortcuts(&args, Some(&config)),
            vec!["bing"]
        );
    }

    #[test]
    fn test_is_config_enabled_respects_aliases() {
        let config = SearchConfig::parse(r#"engine "google" { enabled = false }"#).unwrap();

        assert!(!is_config_enabled(Some(&config), "g"));
        assert!(!is_config_enabled(Some(&config), "google"));
        assert!(is_config_enabled(Some(&config), "ddg"));
    }

    #[test]
    fn test_is_config_enabled_respects_provider_entries() {
        let config = SearchConfig::parse(r#"provider "anysearch" { enabled = false }"#).unwrap();

        assert!(!is_config_enabled(Some(&config), "anysearch"));
        assert!(is_config_enabled(Some(&config), "tavily"));
    }

    #[test]
    fn test_configured_provider_engine_applies_common_settings() {
        let config = SearchConfig::parse(
            r#"
            timeout { value = 17 }
            provider "tavily" {
                api_key = "test-key"
                weight = 1.7
            }
            "#,
        )
        .unwrap();
        let engine = create_provider_engine(BuiltinProvider::Tavily, Some(&config)).unwrap();

        assert_eq!(engine.shortcut(), "tavily");
        assert_eq!(engine.config().timeout, 17);
        assert_eq!(engine.config().weight, 1.7);
        assert!(engine.readiness().is_ready());
    }

    #[test]
    fn test_provider_readiness_summary_never_contains_credentials() {
        assert_eq!(
            provider_readiness_summary(&ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Anonymous,
            }),
            "ready, keyless/anonymous"
        );
        assert_eq!(
            provider_readiness_summary(&ProviderReadiness::MissingCredential {
                environment_variable: Some("TAVILY_API_KEY".to_string()),
            }),
            "not ready, missing TAVILY_API_KEY"
        );
    }

    #[test]
    fn test_json_output_preserves_provider_rich_fields() {
        let mut results = SearchResults::new();
        results.add_result(a3s_search::SearchResult::new(
            "https://example.com",
            "Example",
            "Snippet",
        ));
        results.add_answer("Direct answer");
        results.add_suggestion("Related query");
        results.add_image(
            a3s_search::SearchImage::new("https://example.com/image.png")
                .with_description("Example image"),
        );
        results.add_report(
            a3s_search::SearchReport::new("Tavily")
                .with_provider("tavily")
                .with_request_id("request-1")
                .with_usage(a3s_search::SearchUsage::new().with_credits(1.0)),
        );
        results.set_duration(42);

        let output = json_output("rust", &results, 1);

        assert_eq!(output["query"], "rust");
        assert_eq!(output["answers"], serde_json::json!(["Direct answer"]));
        assert_eq!(output["suggestions"], serde_json::json!(["Related query"]));
        assert_eq!(output["images"][0]["description"], "Example image");
        assert_eq!(output["reports"][0]["provider"], "tavily");
        assert_eq!(output["reports"][0]["request_id"], "request-1");
        assert_eq!(output["reports"][0]["usage"]["credits"], 1.0);
        assert_eq!(output["count"], 1);
    }

    #[test]
    fn test_json_output_preserves_typed_outcomes_and_retry_context() {
        let results: SearchResults = serde_json::from_value(serde_json::json!({
            "results": [],
            "suggestions": [],
            "answers": [],
            "images": [],
            "errors": [["Quota API", "quota exhausted"]],
            "failures": [{
                "engine": "Quota API",
                "provider": "anysearch",
                "kind": "circuit_open",
                "message": "shared engine circuit is open",
                "transient": true,
                "retry_after_seconds": 30
            }],
            "reports": [],
            "outcomes": [{
                "engine": "Quota API",
                "shortcut": "anysearch",
                "provider": "anysearch",
                "kind": "circuit_open",
                "result_count": 0,
                "duration_ms": 0,
                "failure": {
                    "engine": "Quota API",
                    "provider": "anysearch",
                    "kind": "circuit_open",
                    "message": "shared engine circuit is open",
                    "transient": true,
                    "retry_after_seconds": 30
                }
            }],
            "count": 0,
            "duration_ms": 1
        }))
        .expect("typed search result fixture");

        let output = json_output("unrelated query", &results, 10);

        assert_eq!(output["outcomes"][0]["kind"], "circuit_open");
        assert_eq!(output["outcomes"][0]["failure"]["retry_after_seconds"], 30);
        assert_eq!(output["failures"][0]["kind"], "circuit_open");
        assert_eq!(output["failures"][0]["retry_after_seconds"], 30);
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 150), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        let s = "a".repeat(150);
        assert_eq!(truncate_str(&s, 150), s);
    }

    #[test]
    fn test_truncate_str_long_ascii() {
        let s = "a".repeat(200);
        let result = truncate_str(&s, 150);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 153); // 150 + "..."
    }

    #[test]
    fn test_truncate_str_chinese() {
        // Chinese chars are 3 bytes each; truncating at byte 150 must not panic
        let s = "中".repeat(100); // 300 bytes
        let result = truncate_str(&s, 150);
        assert!(result.ends_with("..."));
        // Should truncate at a char boundary (150 / 3 = 50 chars)
        assert!(result.starts_with("中"));
    }

    #[test]
    fn test_truncate_str_mixed_cjk() {
        let s = "Hello世界！This is a test with 中文 and English mixed content that is long enough to be truncated at some point in the middle of the string somewhere around here.";
        let result = truncate_str(s, 150);
        assert!(result.ends_with("..."));
        // Must not panic on mixed content
    }

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 150), "");
    }

    #[test]
    fn test_truncate_str_emoji() {
        // Emoji are 4 bytes each
        let s = "🦀".repeat(50); // 200 bytes
        let result = truncate_str(&s, 150);
        assert!(result.ends_with("..."));
        assert!(result.starts_with("🦀"));
    }
}
