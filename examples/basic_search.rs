//! Example: default product cascade and its structural receipt.

use std::time::Duration;

use a3s_search::product::{self, HeadlessBrowser};
use a3s_search::{EngineTierPlan, SearchQuery};

#[tokio::main]
async fn main() -> a3s_search::Result<()> {
    let plan = EngineTierPlan::builtin_default();
    let outcome = product::execute_cascade(
        &plan,
        product::CascadeRequest {
            query: SearchQuery::new("rust programming"),
            limit: 10,
            timeout: Duration::from_secs(product::DEFAULT_TIMEOUT_SECONDS),
            proxy: None,
            config: None,
            browser: HeadlessBrowser::default(),
            browser_max_retries: 0,
        },
        product::CascadeControls::default(),
    )
    .await?;

    let receipt = &outcome.receipt;
    println!("schema: {}", receipt.schema);
    println!("requirements met: {}", receipt.retrieval_requirements_met);
    println!("results: {}", outcome.results.count);
    for (i, result) in outcome.results.items().iter().take(10).enumerate() {
        println!("{}. {:.3} {}", i + 1, result.score, result.title);
        println!("   URL: {}", result.url);
        println!("   Engines: {:?}", result.engines);
    }

    Ok(())
}
