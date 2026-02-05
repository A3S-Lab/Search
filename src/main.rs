//! A3S Search CLI - Meta search engine command line interface.

use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use a3s_search::{
    engines::{Baidu, BingChina, Brave, DuckDuckGo, Google, So360, Sogou, Wikipedia},
    proxy::{ProxyConfig, ProxyPool, ProxyProtocol},
    Search, SearchQuery,
};

/// A3S Search - Embeddable meta search engine CLI
#[derive(Parser)]
#[command(name = "a3s-search")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Search using meta search engine
    Search(SearchArgs),

    /// List available search engines
    Engines,
}

#[derive(Parser)]
struct SearchArgs {
    /// Search query
    query: String,

    /// Search engines to use (comma-separated)
    /// Available: ddg, brave, google, wiki, baidu, sogou, bing_cn, 360
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
}

#[derive(Clone, Copy, ValueEnum)]
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
        Commands::Search(args) => run_search(args).await,
        Commands::Engines => list_engines(),
    }
}

fn list_engines() -> Result<()> {
    println!("Available search engines:\n");
    println!("  International:");
    println!("    ddg      - DuckDuckGo (privacy-focused search)");
    println!("    brave    - Brave Search");
    println!("    google   - Google Search");
    println!("    wiki     - Wikipedia");
    println!();
    println!("  Chinese (中国搜索引擎):");
    println!("    baidu    - Baidu (百度)");
    println!("    sogou    - Sogou (搜狗)");
    println!("    bing_cn  - Bing China (必应中国)");
    println!("    360      - 360 Search (360搜索)");
    println!();
    println!("Usage: a3s-search search \"query\" -e ddg,wiki,baidu");
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

    // Add engines based on selection
    let engine_shortcuts: Vec<String> = args
        .engines
        .unwrap_or_else(|| vec!["ddg".to_string(), "wiki".to_string()]);

    for shortcut in &engine_shortcuts {
        match shortcut.as_str() {
            "ddg" | "duckduckgo" => search.add_engine(DuckDuckGo::new()),
            "brave" => search.add_engine(Brave::new()),
            "google" | "g" => search.add_engine(Google::new()),
            "wiki" | "wikipedia" => search.add_engine(Wikipedia::new()),
            "baidu" => search.add_engine(Baidu::new()),
            "sogou" => search.add_engine(Sogou::new()),
            "bing_cn" | "bing" => search.add_engine(BingChina::new()),
            "360" | "so360" => search.add_engine(So360::new()),
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
        ProxyProtocol::Http | ProxyProtocol::Https => 8080,
        ProxyProtocol::Socks5 => 1080,
    });

    let mut config = ProxyConfig::new(host, port).with_protocol(protocol);

    if let Some(password) = url.password() {
        config = config.with_auth(url.username(), password);
    }

    Ok(config)
}
