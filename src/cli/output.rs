//! Search result rendering for the CLI.

use anyhow::Result;
use clap::ValueEnum;

use a3s_search::{select_structural_window, SearchCascadeOutcomeV2, SearchResult, SearchResults};

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
    let visible = select_structural_window(results, limit, outcome.receipt.retrieval_requirements);
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

            for (index, result) in visible.iter().enumerate() {
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
            for result in visible {
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
    let requirements = outcome.receipt.retrieval_requirements;
    let visible = select_structural_window(&outcome.results, limit, requirements);
    let visible_health = requirements.evaluate_items(visible.iter().copied());
    payload["results"] = serde_json::to_value(&visible)?;
    payload["count"] = serde_json::json!(visible.len());
    payload["visible_retrieval_requirements_met"] =
        serde_json::json!(requirements.is_met(&visible_health));
    payload["visible_retrieval_health"] = serde_json::to_value(visible_health)?;
    payload["cascade_receipt"] = serde_json::to_value(&outcome.receipt)?;
    payload["cascade_receipt_binding"] = serde_json::to_value(outcome.receipt_binding()?)?;
    Ok(payload)
}

pub(crate) fn json_output(query: &str, results: &SearchResults, limit: usize) -> serde_json::Value {
    let output: Vec<&SearchResult> = results.items().iter().take(limit).collect();
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

    #[test]
    fn cascade_json_selects_a_structurally_healthy_visible_window() {
        let query = SearchQuery::new("opaque query");
        let requirements = RetrievalRequirements::for_limit(5);
        let mut cascade = SearchCascade::new(query.clone(), requirements);
        let mut results = SearchResults::new();
        for (url, engine, score) in [
            ("https://one.example/1", "first", 6.0),
            ("https://one.example/2", "first", 5.0),
            ("https://two.example/3", "second", 4.0),
            ("https://two.example/4", "second", 3.0),
            ("https://two.example/5", "second", 2.0),
            ("https://three.example/6", "first", 1.0),
        ] {
            let mut result = SearchResult::new(url, "opaque", "opaque").with_engine(engine, 1);
            result.score = score;
            results.add_result(result);
        }
        cascade.push_tier("headless", results);
        let outcome = cascade
            .finish_with_tier_plan(["headless", "http_rss", "api"])
            .unwrap();

        let output = cascade_json_output(&query.query, &outcome, 5).unwrap();
        let urls = output["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|result| result["url"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            urls,
            vec![
                "https://one.example/1",
                "https://one.example/2",
                "https://two.example/3",
                "https://two.example/4",
                "https://three.example/6",
            ]
        );
        assert_eq!(output["visible_retrieval_health"]["unique_host_count"], 3);
        assert_eq!(output["visible_retrieval_requirements_met"], true);
        assert_eq!(output["total_count"], 6);
    }
}
