//! Search result rendering for the CLI.

use anyhow::Result;
use clap::ValueEnum;

use a3s_search::SearchResults;

/// CLI output format.
#[derive(Clone, Copy, ValueEnum, Debug)]
pub(crate) enum OutputFormat {
    /// Human-readable text output.
    Text,
    /// Structured JSON output.
    Json,
    /// Compact title and URL lines.
    Compact,
}

pub(crate) fn print_results(
    query: &str,
    results: &SearchResults,
    limit: usize,
    format: OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!(
                "\nSearch results for \"{}\" ({} results in {}ms):\n",
                query, results.count, results.duration_ms
            );

            if !results.answers().is_empty() {
                println!("Answers:");
                for answer in results.answers() {
                    println!("  - {answer}");
                }
                println!();
            }

            for (index, result) in results.items().iter().take(limit).enumerate() {
                let mut engines: Vec<_> = result.engines.iter().collect();
                engines.sort_unstable();
                println!("{}. {}", index + 1, result.title);
                println!("   URL: {}", result.url);
                if !result.content.is_empty() {
                    println!("   {}", truncate_str(&result.content, 150));
                }
                println!("   Engines: {:?} | Score: {:.2}", engines, result.score);
                println!();
            }

            if !results.suggestions().is_empty() {
                println!("Suggestions: {}", results.suggestions().join(", "));
            }
        }
        OutputFormat::Json => {
            let payload = json_output(query, results, limit);
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        OutputFormat::Compact => {
            for result in results.items().iter().take(limit) {
                println!("{}\t{}", result.title, result.url);
            }
        }
    }
    Ok(())
}

pub(crate) fn json_output(query: &str, results: &SearchResults, limit: usize) -> serde_json::Value {
    let output: Vec<_> = results.items().iter().take(limit).collect();
    serde_json::json!({
        "query": query,
        "results": output,
        "answers": results.answers(),
        "suggestions": results.suggestions(),
        "images": results.images(),
        "reports": results.reports(),
        "errors": results.errors(),
        "failures": results.failures(),
        "outcomes": results.outcomes(),
        "count": output.len(),
        "total_count": results.count,
        "duration_ms": results.duration_ms,
    })
}

/// Truncates a string at a valid UTF-8 boundary.
pub(crate) fn truncate_str(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let truncated = match value
        .char_indices()
        .take_while(|(index, _)| *index < max_bytes)
        .last()
    {
        Some((index, character)) => &value[..index + character.len_utf8()],
        None => "",
    };
    format!("{truncated}...")
}
