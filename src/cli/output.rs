//! Search result rendering for the CLI.

use anyhow::Result;
use clap::ValueEnum;

use a3s_search::{SearchCascadeOutcomeV2, SearchResults};

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

pub(crate) fn print_cascade_results(
    query: &str,
    outcome: &SearchCascadeOutcomeV2,
    limit: usize,
    format: OutputFormat,
) -> Result<()> {
    outcome.validate()?;
    let results = &outcome.results;
    match format {
        OutputFormat::Text => {
            let binding = outcome.receipt_binding()?;
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
            let executed = outcome
                .receipt
                .executed_tiers
                .iter()
                .map(|report| report.tier.as_str())
                .collect::<Vec<_>>()
                .join(" -> ");
            println!(
                "Cascade: {} | retrieval requirements: {} | receipt: {}",
                if executed.is_empty() {
                    "none"
                } else {
                    &executed
                },
                if outcome.receipt.retrieval_requirements_met {
                    "met"
                } else {
                    "not met"
                },
                binding.sha256
            );
        }
        OutputFormat::Json => {
            let payload = cascade_json_output(query, outcome, limit)?;
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

pub(crate) fn cascade_json_output(
    query: &str,
    outcome: &SearchCascadeOutcomeV2,
    limit: usize,
) -> Result<serde_json::Value> {
    outcome.validate()?;
    let mut payload = json_output(query, &outcome.results, limit);
    payload["cascade_receipt"] = serde_json::to_value(&outcome.receipt)?;
    payload["cascade_receipt_binding"] = serde_json::to_value(outcome.receipt_binding()?)?;
    Ok(payload)
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

#[cfg(test)]
mod tests {
    use a3s_search::{
        RetrievalRequirements, SearchCascade, SearchQuery, SearchResult, SearchResults,
    };

    use super::*;

    #[test]
    fn cascade_json_binds_the_complete_plan_health_and_results() {
        let query = SearchQuery::new("portable research query");
        let mut cascade = SearchCascade::new(query.clone(), RetrievalRequirements::for_limit(1));
        let mut results = SearchResults::new();
        results.add_result(
            SearchResult::new(
                "https://example.com/research",
                "Portable research query",
                "Independent evidence for a portable research query.",
            )
            .with_engine("fixture", 1),
        );
        cascade.push_tier("headless", results);
        let outcome = cascade.finish_with_tier_plan(["headless", "http_rss", "api"]);
        let output = cascade_json_output(&query.query, &outcome.unwrap(), 1).unwrap();

        assert_eq!(output["cascade_receipt"]["configured_tiers"][0], "headless");
        assert_eq!(
            output["cascade_receipt"]["retrieval_requirements_met"],
            true
        );
        assert_eq!(
            output["cascade_receipt_binding"]["sha256"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
    }
}
