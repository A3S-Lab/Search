//! A3S Search CLI - Meta search engine command line interface.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use a3s_search::{
    engines::{
        Bing, BingParser, Brave, BraveParser, DuckDuckGo, DuckDuckGoParser, So360, So360Parser,
        Sogou, SogouParser, Wikipedia,
    },
    Engine, EngineConfig, HttpFetcher, PageFetcher, SafeSearch, Search, SearchConfig, SearchQuery,
    TimeRange,
};

#[cfg(feature = "headless")]
use a3s_search::{
    browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig},
    engines::{Baidu, BingChina, Google},
    WaitStrategy,
};

/// A3S Search - Embeddable meta search engine CLI
#[derive(Parser)]
#[command(name = "a3s-search")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Search query (if no subcommand is provided)
    query: Option<String>,

    /// Search engines to use (comma-separated)
    /// Available: ddg, brave, bing, wiki, sogou, 360, g, baidu, bing_cn
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
enum OutputFormat {
    /// Human-readable text output
    Text,
    /// JSON output
    Json,
    /// Compact single-line output
    Compact,
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
        Some(Commands::Engines) => list_engines(),
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
                println!("A3S Search - Meta search engine CLI\n");
                println!("Usage: a3s-search <QUERY> [OPTIONS]");
                println!("       a3s-search engines\n");
                println!("Examples:");
                println!("  a3s-search \"Rust programming\"");
                println!("  a3s-search \"Rust\" -e ddg,wiki -l 5");
                println!("  a3s-search \"Rust\" -f json");
                println!("  a3s-search \"Rust\" -p http://127.0.0.1:8080\n");
                println!("Options:");
                println!(
                    "  -e, --engines <ENGINES>  Engines: ddg,brave,bing,wiki,sogou,360,g,baidu,bing_cn"
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

fn list_engines() -> Result<()> {
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

    #[cfg(feature = "headless")]
    {
        println!();
        println!("  Headless (Chrome auto-installed if needed):");
        println!("    g        - Google");
        println!("    baidu    - Baidu (百度)");
        println!("    bing_cn  - Bing China (必应中国)");
    }

    println!();
    println!("Usage: a3s-search \"query\" -e ddg,wiki,sogou");
    Ok(())
}

fn selected_engine_shortcuts(args: &SearchArgs, config: Option<&SearchConfig>) -> Vec<String> {
    if let Some(engines) = &args.engines {
        return engines.clone();
    }

    if let Some(config) = config {
        if !config.engines.is_empty() {
            return config
                .enabled_engines()
                .into_iter()
                .map(str::to_string)
                .collect();
        }
    }

    vec!["ddg".to_string(), "wiki".to_string()]
}

fn is_config_enabled(config: Option<&SearchConfig>, shortcut: &str) -> bool {
    config
        .and_then(|config| config.engine_entry(shortcut))
        .map(|entry| entry.enabled)
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
    let config = match &args.config {
        Some(path) => Some(
            SearchConfig::load(path)
                .map_err(|e| anyhow::anyhow!("Failed to load config {}: {}", path.display(), e))?,
        ),
        None => None,
    };

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

    // Log proxy usage
    if let Some(proxy_url) = &args.proxy {
        if matches!(args.format, OutputFormat::Text) {
            eprintln!("Using proxy: {}", proxy_url);
        }
    }

    // Warn if headless engines are requested without the feature
    #[cfg(not(feature = "headless"))]
    {
        let headless_engines = ["g", "google", "baidu", "bing_cn"];
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

    // Create shared HTTP fetcher (with proxy if provided)
    let http_fetcher: std::sync::Arc<dyn PageFetcher> = if let Some(proxy_url) = &args.proxy {
        std::sync::Arc::new(
            HttpFetcher::with_proxy(proxy_url)
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP fetcher with proxy: {}", e))?,
        )
    } else {
        std::sync::Arc::new(HttpFetcher::new())
    };

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
            #[cfg(feature = "headless")]
            "g" | "google" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> = std::sync::Arc::new(
                    BrowserFetcher::new(std::sync::Arc::clone(&browser_pool)).with_wait(
                        WaitStrategy::Selector {
                            css: "div.g".to_string(),
                            timeout_ms: 5000,
                        },
                    ),
                );
                let engine = Google::new(fetcher);
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            #[cfg(feature = "headless")]
            "baidu" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> = std::sync::Arc::new(
                    BrowserFetcher::new(std::sync::Arc::clone(&browser_pool)).with_wait(
                        WaitStrategy::Selector {
                            css: "div.c-container".to_string(),
                            timeout_ms: 5000,
                        },
                    ),
                );
                let engine = Baidu::new(fetcher);
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            #[cfg(feature = "headless")]
            "bing_cn" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> = std::sync::Arc::new(
                    BrowserFetcher::new(std::sync::Arc::clone(&browser_pool))
                        .with_wait(WaitStrategy::Delay { ms: 2000 }),
                );
                let engine = BingChina::new(fetcher);
                let engine_config =
                    configured_engine_config(config.as_ref(), engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            #[cfg(not(feature = "headless"))]
            "g" | "google" | "baidu" | "bing_cn" => {
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
    let results = search.search(query).await?;

    // Show engine errors to the user
    for (engine, error) in results.errors() {
        eprintln!("Warning: {} engine failed: {}", engine, error);
    }

    // Output results
    match args.format {
        OutputFormat::Text => {
            println!(
                "\nSearch results for \"{}\" ({} results in {}ms):\n",
                args.query, results.count, results.duration_ms
            );

            for (i, result) in results.items().iter().take(args.limit).enumerate() {
                println!("{}. {}", i + 1, result.title);
                println!("   URL: {}", result.url);
                if !result.content.is_empty() {
                    let content = truncate_str(&result.content, 150);
                    println!("   {}", content);
                }
                println!(
                    "   Engines: {:?} | Score: {:.2}",
                    result.engines, result.score
                );
                println!();
            }
        }
        OutputFormat::Json => {
            let output: Vec<_> = results.items().iter().take(args.limit).collect();
            let payload = serde_json::json!({
                "results": output,
                "errors": results.errors(),
                "count": output.len(),
                "total_count": results.count,
                "duration_ms": results.duration_ms,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        OutputFormat::Compact => {
            for result in results.items().iter().take(args.limit) {
                println!("{}\t{}", result.title, result.url);
            }
        }
    }

    Ok(())
}

/// Truncates a string to at most `max_bytes` bytes at a valid UTF-8 char boundary.
fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // Find the last char boundary at or before max_bytes
    let truncated = match s.char_indices().take_while(|(i, _)| *i < max_bytes).last() {
        Some((i, c)) => &s[..i + c.len_utf8()],
        None => "",
    };
    format!("{}...", truncated)
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
        let cli = Cli::parse_from(["a3s-search", "query", "-e", "ddg,wiki"]);
        assert_eq!(
            cli.engines,
            Some(vec!["ddg".to_string(), "wiki".to_string()])
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
            "#,
        )
        .unwrap();

        let selected = selected_engine_shortcuts(&args, Some(&config));
        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&"ddg".to_string()));
        assert!(selected.contains(&"wiki".to_string()));
        assert!(!selected.contains(&"brave".to_string()));
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
