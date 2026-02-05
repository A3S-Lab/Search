//! Example: Meta search with Chinese search engines.

use a3s_search::{Search, SearchQuery, engines::{Baidu, Sogou, BingChina, So360}};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    // Create a new search instance with Chinese engines
    let mut search = Search::new();

    // Add Chinese search engines
    search.add_engine(Baidu::new());      // 百度
    search.add_engine(Sogou::new());      // 搜狗
    search.add_engine(BingChina::new());  // 必应中国
    search.add_engine(So360::new());      // 360搜索

    println!("Configured {} Chinese search engines", search.engine_count());

    // Create a search query in Chinese
    let query = SearchQuery::new("Rust 编程语言");

    println!("Searching for: {}", query.query);
    println!();

    // Perform the search
    let results = search.search(query).await?;

    println!("Found {} results in {}ms", results.count, results.duration_ms);
    println!();

    // Display results
    for (i, result) in results.items().iter().take(10).enumerate() {
        println!("{}. {}", i + 1, result.title);
        println!("   URL: {}", result.url);
        println!("   Engines: {:?}", result.engines);
        println!("   Score: {:.2}", result.score);
        if !result.content.is_empty() {
            let snippet = if result.content.chars().count() > 80 {
                result.content.chars().take(80).collect::<String>() + "..."
            } else {
                result.content.clone()
            };
            println!("   {}", snippet);
        }
        println!();
    }

    Ok(())
}
