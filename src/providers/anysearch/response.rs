//! AnySearch MCP response wire types and bounded result parsing.

use serde::Deserialize;
use serde_json::Value;

use super::super::protocol::{
    non_empty, sanitize_provider_multiline_text, sanitize_provider_text, validated_web_url,
};
use super::super::ProviderResult;
use super::PROVIDER_ID;
use crate::{ProviderError, ProviderErrorKind, Result, SearchError};

const MAX_TITLE_CHARS: usize = 512;
const MAX_SNIPPET_CHARS: usize = 2_000;
const MAX_FULL_TEXT_CHARS: usize = 64 * 1024;
const MAX_DATE_CHARS: usize = 128;

#[derive(Deserialize)]
pub(super) struct AnySearchRpcEnvelope {
    pub(super) jsonrpc: Option<String>,
    pub(super) id: Option<Value>,
    pub(super) result: Option<AnySearchToolResult>,
    pub(super) error: Option<AnySearchRpcError>,
}

#[derive(Deserialize)]
pub(super) struct AnySearchToolResult {
    #[serde(rename = "_meta")]
    pub(super) meta: Option<AnySearchMetadata>,
    #[serde(default)]
    pub(super) content: Vec<AnySearchContent>,
    #[serde(rename = "structuredContent")]
    pub(super) structured_content: Option<Value>,
    #[serde(default, rename = "isError")]
    pub(super) is_error: bool,
}

#[derive(Deserialize)]
pub(super) struct AnySearchMetadata {
    pub(super) request_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct AnySearchContent {
    #[serde(rename = "type")]
    content_type: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct AnySearchRpcError {
    pub(super) code: i64,
    pub(super) message: Option<String>,
}

#[derive(Debug)]
pub(super) struct ParsedAnySearch {
    pub(super) results: Vec<ProviderResult>,
    pub(super) total_results: Option<u64>,
    pub(super) response_time_ms: Option<u64>,
}

#[derive(Deserialize)]
struct AnySearchStructuredBody {
    results: Option<Vec<AnySearchStructuredResult>>,
    total_results: Option<u64>,
    response_time_ms: Option<f64>,
    metadata: Option<AnySearchStructuredMetadata>,
}

#[derive(Deserialize)]
struct AnySearchStructuredMetadata {
    total_results: Option<u64>,
    response_time_ms: Option<f64>,
    search_time_ms: Option<f64>,
}

#[derive(Deserialize)]
struct AnySearchStructuredResult {
    title: Option<String>,
    url: Option<String>,
    snippet: Option<String>,
    content: Option<String>,
    full_text: Option<String>,
    score: Option<f64>,
    thumbnail: Option<String>,
    published_date: Option<String>,
    favicon: Option<String>,
}

pub(super) fn parse_structured_content(value: &Value, max_results: u8) -> Option<ParsedAnySearch> {
    let candidate = value.get("data").unwrap_or(value);
    let body: AnySearchStructuredBody = serde_json::from_value(candidate.clone()).ok()?;
    let raw_results = body.results?;
    let raw_result_count = raw_results.len();
    let results: Vec<_> = raw_results
        .into_iter()
        .filter_map(adapt_structured_result)
        .take(usize::from(max_results))
        .collect();
    if raw_result_count > 0 && results.is_empty() {
        return None;
    }

    let metadata = body.metadata;
    let total_results = body.total_results.or_else(|| {
        metadata
            .as_ref()
            .and_then(|metadata| metadata.total_results)
    });
    let response_time_ms = body
        .response_time_ms
        .or_else(|| {
            metadata
                .as_ref()
                .and_then(|metadata| metadata.response_time_ms)
        })
        .or_else(|| {
            metadata
                .as_ref()
                .and_then(|metadata| metadata.search_time_ms)
        })
        .and_then(duration_ms);
    Some(ParsedAnySearch {
        results,
        total_results: total_results.or(Some(raw_result_count as u64)),
        response_time_ms,
    })
}

fn adapt_structured_result(raw: AnySearchStructuredResult) -> Option<ProviderResult> {
    let url = validated_web_url(&raw.url?)?;
    let title = bounded_non_empty(raw.title?, MAX_TITLE_CHARS)?;
    let content = non_empty(raw.content);
    let snippet = non_empty(raw.snippet)
        .map(|snippet| sanitize_provider_text(&snippet, MAX_SNIPPET_CHARS))
        .or_else(|| content.as_deref().map(snippet_from_content))
        .unwrap_or_default();
    let mut result = ProviderResult::new(url, title, snippet);
    if let Some(full_text) = non_empty(raw.full_text).or(content) {
        result.full_text = Some(sanitize_provider_multiline_text(
            &full_text,
            MAX_FULL_TEXT_CHARS,
        ));
    }
    if let Some(score) = raw.score.filter(|score| score.is_finite()) {
        result.relevance_score = Some(score.clamp(0.0, 1.0));
    }
    result.thumbnail = raw.thumbnail.as_deref().and_then(validated_web_url);
    result.published_date = non_empty(raw.published_date)
        .map(|date| sanitize_provider_text(&date, MAX_DATE_CHARS))
        .filter(|date| !date.is_empty());
    result.favicon = raw.favicon.as_deref().and_then(validated_web_url);
    Some(result)
}

pub(super) fn first_text_content(content: &[AnySearchContent]) -> Option<&str> {
    content.iter().find_map(|item| {
        (item.content_type.as_deref() == Some("text"))
            .then_some(item.text.as_deref())
            .flatten()
    })
}

pub(super) fn parse_search_markdown(markdown: &str, max_results: u8) -> Result<ParsedAnySearch> {
    let header = markdown
        .lines()
        .find(|line| line.trim_start().starts_with("## Search Results"))
        .ok_or_else(|| {
            ProviderError::new(
                PROVIDER_ID,
                ProviderErrorKind::InvalidResponse,
                "AnySearch text result did not contain a search-results header",
            )
        })?;
    let (total_results, response_time_ms) = parse_search_stats(header);

    let mut raw_results = Vec::new();
    let mut current: Option<MarkdownResult> = None;
    for line in markdown.lines() {
        let line = line.trim();
        if let Some(title) = numbered_result_title(line) {
            if let Some(result) = current.take() {
                raw_results.push(result);
            }
            current = Some(MarkdownResult {
                title: title.to_string(),
                url: None,
                content: Vec::new(),
            });
            continue;
        }
        let Some(result) = current.as_mut() else {
            continue;
        };
        if let Some(url) = line.strip_prefix("- **URL**:") {
            result.url = Some(url.trim().to_string());
        } else if let Some(content) = line.strip_prefix("- ") {
            if !content.trim().is_empty() {
                result.content.push(content.trim().to_string());
            }
        }
    }
    if let Some(result) = current {
        raw_results.push(result);
    }

    let raw_result_count = raw_results.len();
    let results: Vec<_> = raw_results
        .into_iter()
        .filter_map(adapt_markdown_result)
        .take(usize::from(max_results))
        .collect();
    if total_results.unwrap_or(raw_result_count as u64) > 0 && results.is_empty() {
        return Err(invalid_response(
            "AnySearch returned results without valid web URLs and titles",
        ));
    }

    Ok(ParsedAnySearch {
        results,
        total_results: total_results.or(Some(raw_result_count as u64)),
        response_time_ms,
    })
}

#[derive(Debug)]
struct MarkdownResult {
    title: String,
    url: Option<String>,
    content: Vec<String>,
}

fn adapt_markdown_result(raw: MarkdownResult) -> Option<ProviderResult> {
    let url = validated_web_url(raw.url.as_deref()?)?;
    let title = bounded_non_empty(raw.title, MAX_TITLE_CHARS)?;
    let content = sanitize_provider_multiline_text(&raw.content.join("\n"), MAX_FULL_TEXT_CHARS);
    let snippet = snippet_from_content(&content);
    let mut result = ProviderResult::new(url, title, snippet);
    if !content.is_empty() {
        result.full_text = Some(content);
    }
    Some(result)
}

fn numbered_result_title(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("### ")?;
    let (number, title) = rest.split_once(". ")?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let title = title.trim();
    (!title.is_empty()).then_some(title)
}

fn parse_search_stats(header: &str) -> (Option<u64>, Option<u64>) {
    let Some(start) = header.find('(') else {
        return (None, None);
    };
    let Some(end_offset) = header[start + 1..].find(')') else {
        return (None, None);
    };
    let stats = &header[start + 1..start + 1 + end_offset];
    let mut total_results = None;
    let mut response_time_ms = None;
    for item in stats.split(',').map(str::trim) {
        if let Some(number) = item
            .strip_suffix("results")
            .or_else(|| item.strip_suffix("result"))
            .map(str::trim)
        {
            total_results = number.parse().ok();
        } else if let Some(number) = item.strip_suffix("ms").map(str::trim) {
            response_time_ms = number.parse::<f64>().ok().and_then(duration_ms);
        }
    }
    (total_results, response_time_ms)
}

fn snippet_from_content(content: &str) -> String {
    sanitize_provider_text(content, MAX_SNIPPET_CHARS)
}

fn bounded_non_empty(value: String, max_chars: usize) -> Option<String> {
    let value = sanitize_provider_text(&value, max_chars);
    (!value.is_empty()).then_some(value)
}

fn duration_ms(value: f64) -> Option<u64> {
    const U64_UPPER_BOUND_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;

    let rounded = value.round();
    if (0.0..U64_UPPER_BOUND_EXCLUSIVE).contains(&rounded) {
        Some(rounded as u64)
    } else {
        None
    }
}

fn invalid_response(message: &str) -> SearchError {
    ProviderError::new(PROVIDER_ID, ProviderErrorKind::InvalidResponse, message).into()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn structured_results_are_bounded_by_requested_limit_and_field_limits() {
        let long_snippet = "x".repeat(MAX_SNIPPET_CHARS + 100);
        let long_date = "d".repeat(512);
        let value = json!({
            "results": [
                {
                    "title": "First",
                    "url": "https://first.example/",
                    "snippet": long_snippet,
                    "published_date": long_date
                },
                {
                    "title": "Second",
                    "url": "https://second.example/",
                    "snippet": "second"
                }
            ]
        });

        let parsed = parse_structured_content(&value, 1).unwrap();

        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.results[0].snippet.chars().count(), MAX_SNIPPET_CHARS);
        assert!(
            parsed.results[0]
                .published_date
                .as_deref()
                .unwrap()
                .chars()
                .count()
                <= 128
        );
        assert_eq!(parsed.total_results, Some(2));
    }

    #[test]
    fn markdown_limit_counts_valid_results_not_invalid_entries() {
        let parsed = parse_search_markdown(
            "## Search Results (2 results, 1ms)\n\
             ### 1. Invalid\n\
             - **URL**: javascript:alert(1)\n\
             - invalid\n\
             ### 2. Valid\n\
             - **URL**: https://valid.example/\n\
             - valid\n",
            1,
        )
        .unwrap();

        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.results[0].title, "Valid");
    }

    #[test]
    fn response_duration_conversion_never_saturates() {
        assert_eq!(duration_ms(1.5), Some(2));
        assert_eq!(duration_ms(18_446_744_073_709_551_616.0), None);
        assert_eq!(duration_ms(f64::INFINITY), None);
    }
}
