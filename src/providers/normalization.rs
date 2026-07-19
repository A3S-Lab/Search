//! Defensive normalization for all provider-controlled rich output.

use serde_json::{Map, Value};

use super::protocol::{
    sanitize_provider_multiline_text, sanitize_provider_text, validated_web_url,
};
use super::{ProviderResponse, ProviderResult};
use crate::SearchImage;

const MAX_RESULTS: usize = 100;
const MAX_TITLE_CHARS: usize = 512;
const MAX_SNIPPET_CHARS: usize = 2_000;
const MAX_FULL_TEXT_CHARS: usize = 256 * 1024;
const MAX_DATE_CHARS: usize = 128;
const MAX_RESULT_IMAGES: usize = 32;
const MAX_QUERY_IMAGES: usize = 64;
const MAX_IMAGE_DESCRIPTION_CHARS: usize = 1_000;
const MAX_ANSWERS: usize = 16;
const MAX_ANSWER_CHARS: usize = 16 * 1024;
const MAX_SUGGESTIONS: usize = 32;
const MAX_SUGGESTION_CHARS: usize = 512;
const NORMALIZATION_METADATA_KEY: &str = "_a3s_normalization";

#[derive(Debug, Default)]
struct NormalizationSummary {
    dropped_results: usize,
    dropped_images: usize,
    dropped_answers: usize,
    dropped_suggestions: usize,
    invalid_links: usize,
    invalid_relevance_scores: usize,
    truncated_fields: usize,
}

impl NormalizationSummary {
    fn changed(&self) -> bool {
        self.dropped_results > 0
            || self.dropped_images > 0
            || self.dropped_answers > 0
            || self.dropped_suggestions > 0
            || self.invalid_links > 0
            || self.invalid_relevance_scores > 0
            || self.truncated_fields > 0
    }

    fn into_value(self) -> Value {
        let mut value = Map::new();
        value.insert("changed".to_string(), Value::Bool(true));
        insert_count(&mut value, "dropped_results", self.dropped_results);
        insert_count(&mut value, "dropped_images", self.dropped_images);
        insert_count(&mut value, "dropped_answers", self.dropped_answers);
        insert_count(&mut value, "dropped_suggestions", self.dropped_suggestions);
        insert_count(&mut value, "invalid_links", self.invalid_links);
        insert_count(
            &mut value,
            "invalid_relevance_scores",
            self.invalid_relevance_scores,
        );
        insert_count(&mut value, "truncated_fields", self.truncated_fields);
        Value::Object(value)
    }
}

fn insert_count(target: &mut Map<String, Value>, key: &str, count: usize) {
    if count > 0 {
        target.insert(key.to_string(), Value::from(count as u64));
    }
}

pub(super) fn normalize_provider_response(mut response: ProviderResponse) -> ProviderResponse {
    let mut summary = NormalizationSummary::default();
    response.results = normalize_results(response.results, &mut summary);

    let (answers, dropped_answers) = normalize_text_items(
        response.answers,
        MAX_ANSWERS,
        MAX_ANSWER_CHARS,
        &mut summary,
    );
    response.answers = answers;
    summary.dropped_answers = dropped_answers;

    let (suggestions, dropped_suggestions) = normalize_text_items(
        response.suggestions,
        MAX_SUGGESTIONS,
        MAX_SUGGESTION_CHARS,
        &mut summary,
    );
    response.suggestions = suggestions;
    summary.dropped_suggestions = dropped_suggestions;

    response.images = normalize_images(response.images, MAX_QUERY_IMAGES, &mut summary);

    if summary.changed() {
        response
            .report
            .metadata
            .insert(NORMALIZATION_METADATA_KEY.to_string(), summary.into_value());
    }
    response
}

fn normalize_results(
    results: Vec<ProviderResult>,
    summary: &mut NormalizationSummary,
) -> Vec<ProviderResult> {
    let mut normalized = Vec::with_capacity(results.len().min(MAX_RESULTS));
    let mut results = results.into_iter();

    while let Some(result) = results.next() {
        if normalized.len() >= MAX_RESULTS {
            summary.dropped_results = summary
                .dropped_results
                .saturating_add(1)
                .saturating_add(results.count());
            break;
        }
        match normalize_result(result, summary) {
            Some(result) => normalized.push(result),
            None => summary.dropped_results = summary.dropped_results.saturating_add(1),
        }
    }
    normalized
}

fn normalize_result(
    result: ProviderResult,
    summary: &mut NormalizationSummary,
) -> Option<ProviderResult> {
    let url = normalize_url(&result.url, summary)?;
    let title = bounded_single_line(result.title, MAX_TITLE_CHARS, summary);
    if title.is_empty() {
        return None;
    }
    let snippet = bounded_single_line(result.snippet, MAX_SNIPPET_CHARS, summary);
    let full_text = result.full_text.and_then(|full_text| {
        let full_text = bounded_multiline(full_text, MAX_FULL_TEXT_CHARS, summary);
        (!full_text.is_empty()).then_some(full_text)
    });
    let relevance_score = match result.relevance_score {
        Some(score) if score.is_finite() => {
            if !(0.0..=1.0).contains(&score) {
                summary.invalid_relevance_scores =
                    summary.invalid_relevance_scores.saturating_add(1);
            }
            Some(score.clamp(0.0, 1.0))
        }
        Some(_) => {
            summary.invalid_relevance_scores = summary.invalid_relevance_scores.saturating_add(1);
            None
        }
        None => None,
    };
    let thumbnail = normalize_optional_url(result.thumbnail, summary);
    let published_date = result.published_date.and_then(|date| {
        let date = bounded_single_line(date, MAX_DATE_CHARS, summary);
        (!date.is_empty()).then_some(date)
    });
    let favicon = normalize_optional_url(result.favicon, summary);
    let images = normalize_images(result.images, MAX_RESULT_IMAGES, summary);

    let mut normalized = ProviderResult::new(url, title, snippet);
    normalized.result_type = result.result_type;
    normalized.full_text = full_text;
    normalized.relevance_score = relevance_score;
    normalized.thumbnail = thumbnail;
    normalized.published_date = published_date;
    normalized.favicon = favicon;
    normalized.images = images;
    Some(normalized)
}

fn normalize_text_items(
    values: Vec<String>,
    max_items: usize,
    max_chars: usize,
    summary: &mut NormalizationSummary,
) -> (Vec<String>, usize) {
    let mut normalized = Vec::with_capacity(values.len().min(max_items));
    let mut dropped = 0usize;
    let mut values = values.into_iter();

    while let Some(value) = values.next() {
        if normalized.len() >= max_items {
            dropped = dropped.saturating_add(1).saturating_add(values.count());
            break;
        }
        let value = bounded_single_line(value, max_chars, summary);
        if value.is_empty() || normalized.contains(&value) {
            dropped = dropped.saturating_add(1);
        } else {
            normalized.push(value);
        }
    }
    (normalized, dropped)
}

fn normalize_images(
    images: Vec<SearchImage>,
    max_images: usize,
    summary: &mut NormalizationSummary,
) -> Vec<SearchImage> {
    let mut normalized = Vec::with_capacity(images.len().min(max_images));
    let mut images = images.into_iter();

    while let Some(image) = images.next() {
        if normalized.len() >= max_images {
            summary.dropped_images = summary
                .dropped_images
                .saturating_add(1)
                .saturating_add(images.count());
            break;
        }
        let Some(url) = normalize_url(&image.url, summary) else {
            summary.dropped_images = summary.dropped_images.saturating_add(1);
            continue;
        };
        let description = image.description.and_then(|description| {
            let description =
                bounded_single_line(description, MAX_IMAGE_DESCRIPTION_CHARS, summary);
            (!description.is_empty()).then_some(description)
        });
        crate::result::merge_image(&mut normalized, SearchImage { url, description });
    }
    normalized
}

fn normalize_optional_url(
    value: Option<String>,
    summary: &mut NormalizationSummary,
) -> Option<String> {
    value.and_then(|value| normalize_url(&value, summary))
}

fn normalize_url(value: &str, summary: &mut NormalizationSummary) -> Option<String> {
    let url = validated_web_url(value);
    if url.is_none() {
        summary.invalid_links = summary.invalid_links.saturating_add(1);
    }
    url
}

fn bounded_single_line(
    value: String,
    max_chars: usize,
    summary: &mut NormalizationSummary,
) -> String {
    if exceeds_char_limit(&value, max_chars) {
        summary.truncated_fields = summary.truncated_fields.saturating_add(1);
    }
    sanitize_provider_text(&value, max_chars)
}

fn bounded_multiline(
    value: String,
    max_chars: usize,
    summary: &mut NormalizationSummary,
) -> String {
    if exceeds_char_limit(&value, max_chars) {
        summary.truncated_fields = summary.truncated_fields.saturating_add(1);
    }
    sanitize_provider_multiline_text(&value, max_chars)
}

fn exceeds_char_limit(value: &str, max_chars: usize) -> bool {
    value.chars().nth(max_chars).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ProviderReport, ProviderResponse, ProviderResult};

    #[test]
    fn valid_small_response_does_not_add_adapter_metadata() {
        let response = ProviderResponse::new()
            .with_result(ProviderResult::new(
                "https://example.com/result",
                "Example",
                "Snippet",
            ))
            .with_answer("Answer")
            .with_suggestion("Suggestion")
            .with_image(SearchImage::new("https://example.com/image.png"));

        let normalized = normalize_provider_response(response);

        assert_eq!(normalized.results.len(), 1);
        assert!(!normalized
            .report
            .metadata
            .contains_key(NORMALIZATION_METADATA_KEY));
    }

    #[test]
    fn result_and_collection_limits_are_reported() {
        let results = (0..=MAX_RESULTS)
            .map(|index| {
                ProviderResult::new(
                    format!("https://example.com/{index}"),
                    format!("Result {index}"),
                    "Snippet",
                )
            })
            .collect();
        let response = ProviderResponse {
            results,
            answers: (0..=MAX_ANSWERS)
                .map(|index| format!("Answer {index}"))
                .collect(),
            suggestions: (0..=MAX_SUGGESTIONS)
                .map(|index| format!("Suggestion {index}"))
                .collect(),
            images: (0..=MAX_QUERY_IMAGES)
                .map(|index| SearchImage::new(format!("https://example.com/{index}.png")))
                .collect(),
            report: ProviderReport::new(),
        };

        let normalized = normalize_provider_response(response);
        let report = &normalized.report.metadata[NORMALIZATION_METADATA_KEY];

        assert_eq!(normalized.results.len(), MAX_RESULTS);
        assert_eq!(normalized.answers.len(), MAX_ANSWERS);
        assert_eq!(normalized.suggestions.len(), MAX_SUGGESTIONS);
        assert_eq!(normalized.images.len(), MAX_QUERY_IMAGES);
        assert_eq!(report["dropped_results"], 1);
        assert_eq!(report["dropped_answers"], 1);
        assert_eq!(report["dropped_suggestions"], 1);
        assert_eq!(report["dropped_images"], 1);
    }
}
