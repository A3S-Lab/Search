//! Example: Basic meta search across multiple engines.

use a3s_search::{Search, SearchQuery, engines::{DuckDuckGo, Wikipedia}};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    // Create a new search instance
    let mut search = Search::new();

    // Add search engines
    search.add_engine(DuckDuckGo::new());
    search.add_engine(Wikipedia::new());

    println!("Configured {} search engines", search.engine_count());

    // Create a search query
    let query = SearchQuery::new("rust programming language");

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
            let snippet = if result.content.len() > 100 {
                format!("{}...", &result.content[..100])
            } else {
                result.content.clone()
            };
            println!("   {}", snippet);
        }
        println!();
    }

    Ok(())
}
