//! A3S Search CLI - Meta search engine command line interface.

use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use a3s_search::{
    engines::{Brave, DuckDuckGo, So360, Sogou, Wikipedia},
    proxy::{ProxyConfig, ProxyPool, ProxyProtocol},
    Search, SearchQuery,
};

#[cfg(feature = "headless")]
use a3s_search::{
    browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig},
    engines::{Baidu, BingChina, Google},
    PageFetcher, WaitStrategy,
};

/// A3S Search - Embeddable meta search engine CLI
#[derive(Parser)]
#[command(name = "a3s-search")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Search query (if no subcommand is provided)
    query: Option<String>,

    /// Search engines to use (comma-separated)
    /// Available: ddg, brave, wiki, sogou, 360, g, baidu, bing_cn
    #[arg(short, long, value_delimiter = ',')]
    engines: Option<Vec<String>>,

    /// Maximum number of results to display
    #[arg(short, long, default_value = "10")]
    limit: usize,

    /// Search timeout in seconds
    #[arg(short, long, default_value = "10")]
    timeout: u64,

    /// Output format
    #[arg(short, long, default_value = "text")]
    format: OutputFormat,

    /// Proxy URL (e.g., http://127.0.0.1:8080 or socks5://127.0.0.1:1080)
    #[arg(short, long)]
    proxy: Option<String>,

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
                    format: cli.format,
                    proxy: cli.proxy,
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
                println!("  -e, --engines <ENGINES>  Engines: ddg,brave,wiki,sogou,360,g,baidu,bing_cn");
                println!("  -l, --limit <N>          Max results (default: 10)");
                println!("  -t, --timeout <SECS>     Timeout in seconds (default: 10)");
                println!("  -f, --format <FORMAT>    Output: text, json, compact");
                println!("  -p, --proxy <URL>        Proxy URL (http/https/socks5)");
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
    timeout: u64,
    format: OutputFormat,
    proxy: Option<String>,
}

fn list_engines() -> Result<()> {
    println!("Available search engines:\n");
    println!("  International:");
    println!("    ddg      - DuckDuckGo (privacy-focused search)");
    println!("    brave    - Brave Search");
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

async fn run_search(args: SearchArgs) -> Result<()> {
    let mut search = Search::new();
    search.set_timeout(Duration::from_secs(args.timeout));

    // Setup proxy if provided
    if let Some(proxy_url) = &args.proxy {
        let proxy_config = parse_proxy_url(proxy_url)?;
        let proxy_pool = ProxyPool::with_proxies(vec![proxy_config]);
        search.set_proxy_pool(proxy_pool);
        if matches!(args.format, OutputFormat::Text) {
            eprintln!("Using proxy: {}", proxy_url);
        }
    }

    // Warn if headless engines are requested without the feature
    #[cfg(not(feature = "headless"))]
    {
        let engine_list = args.engines.as_deref().unwrap_or(&[]);
        let headless_engines = ["g", "google", "baidu", "bing_cn", "bing"];
        for e in engine_list {
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

    // Add engines based on selection
    let engine_shortcuts: Vec<String> = args
        .engines
        .unwrap_or_else(|| vec!["ddg".to_string(), "wiki".to_string()]);

    for shortcut in &engine_shortcuts {
        match shortcut.as_str() {
            "ddg" | "duckduckgo" => search.add_engine(DuckDuckGo::new()),
            "brave" => search.add_engine(Brave::new()),
            "wiki" | "wikipedia" => search.add_engine(Wikipedia::new()),
            "sogou" => search.add_engine(Sogou::new()),
            "360" | "so360" => search.add_engine(So360::new()),
            #[cfg(feature = "headless")]
            "g" | "google" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> =
                    std::sync::Arc::new(BrowserFetcher::new(std::sync::Arc::clone(&browser_pool)).with_wait(
                        WaitStrategy::Selector {
                            css: "div.g".to_string(),
                            timeout_ms: 5000,
                        },
                    ));
                search.add_engine(Google::new(fetcher));
            }
            #[cfg(feature = "headless")]
            "baidu" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> =
                    std::sync::Arc::new(BrowserFetcher::new(std::sync::Arc::clone(&browser_pool)).with_wait(
                        WaitStrategy::Selector {
                            css: "div.c-container".to_string(),
                            timeout_ms: 5000,
                        },
                    ));
                search.add_engine(Baidu::new(fetcher));
            }
            #[cfg(feature = "headless")]
            "bing_cn" | "bing" => {
                let fetcher: std::sync::Arc<dyn PageFetcher> =
                    std::sync::Arc::new(BrowserFetcher::new(std::sync::Arc::clone(&browser_pool)).with_wait(
                        WaitStrategy::Delay { ms: 2000 },
                    ));
                search.add_engine(BingChina::new(fetcher));
            }
            #[cfg(not(feature = "headless"))]
            "g" | "google" | "baidu" | "bing_cn" | "bing" => {
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
    let query = SearchQuery::new(&args.query);
    let results = search.search(query).await?;

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
                    let content = if result.content.len() > 150 {
                        format!("{}...", &result.content[..150])
                    } else {
                        result.content.clone()
                    };
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
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Compact => {
            for result in results.items().iter().take(args.limit) {
                println!("{}\t{}", result.title, result.url);
            }
        }
    }

    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

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
        assert_eq!(cli.timeout, 10);
        assert!(cli.proxy.is_none());
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
        assert_eq!(cli.timeout, 30);
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
        assert_eq!(cli.timeout, 15);
        assert!(matches!(cli.format, OutputFormat::Json));
        assert_eq!(cli.proxy, Some("socks5://localhost:1080".to_string()));
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
        assert_eq!(
            cli.engines,
            Some(vec!["g".to_string(), "ddg".to_string()])
        );
    }
}
