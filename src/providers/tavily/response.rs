//! Tavily response wire types and bounded result adaptation.

use serde::Deserialize;
use serde_json::Value;

use super::super::protocol::{
    non_empty, sanitize_provider_multiline_text, sanitize_provider_text, validated_web_url,
};
use super::super::ProviderResult;
use super::PROVIDER_ID;
use crate::{ProviderError, ProviderErrorKind, Result, SearchImage};

const MAX_TITLE_CHARS: usize = 512;
const MAX_SNIPPET_CHARS: usize = 2_000;
const MAX_FULL_TEXT_CHARS: usize = 256 * 1024;
const MAX_DATE_CHARS: usize = 128;
const MAX_IMAGE_DESCRIPTION_CHARS: usize = 1_000;

#[derive(Deserialize)]
pub(super) struct TavilyResponse {
    pub(super) answer: Option<String>,
    #[serde(default)]
    pub(super) results: Vec<TavilyResult>,
    #[serde(default)]
    pub(super) images: Vec<TavilyImage>,
    pub(super) response_time: Option<FlexibleNumber>,
    pub(super) auto_parameters: Option<Value>,
    pub(super) usage: Option<TavilyUsage>,
    pub(super) request_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct TavilyResult {
    title: Option<String>,
    url: Option<String>,
    content: Option<String>,
    score: Option<f64>,
    raw_content: Option<String>,
    published_date: Option<String>,
    favicon: Option<String>,
    #[serde(default)]
    images: Vec<TavilyImage>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum TavilyImage {
    Url(String),
    Rich {
        url: Option<String>,
        description: Option<String>,
    },
}

impl TavilyImage {
    fn adapt(self) -> Option<SearchImage> {
        match self {
            Self::Url(url) => validated_web_url(&url).map(SearchImage::new),
            Self::Rich { url, description } => {
                let url = validated_web_url(url.as_deref()?)?;
                let mut image = SearchImage::new(url);
                if let Some(description) = non_empty(description) {
                    let description =
                        sanitize_provider_text(&description, MAX_IMAGE_DESCRIPTION_CHARS);
                    if !description.is_empty() {
                        image.description = Some(description);
                    }
                }
                Some(image)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum FlexibleNumber {
    Number(f64),
    String(String),
}

impl FlexibleNumber {
    pub(super) fn seconds_to_ms(self) -> Option<u64> {
        const U64_UPPER_BOUND_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;

        let seconds = match self {
            Self::Number(value) => value,
            Self::String(value) => value.trim().parse().ok()?,
        };
        let milliseconds = (seconds * 1_000.0).round();
        if (0.0..U64_UPPER_BOUND_EXCLUSIVE).contains(&milliseconds) {
            Some(milliseconds as u64)
        } else {
            None
        }
    }
}

#[derive(Deserialize)]
pub(super) struct TavilyUsage {
    credits: Option<f64>,
}

impl TavilyUsage {
    pub(super) fn credits(self) -> Option<f64> {
        self.credits
            .filter(|credits| credits.is_finite() && *credits >= 0.0)
    }
}

pub(super) fn adapt_results(
    raw_results: Vec<TavilyResult>,
    max_results: u8,
) -> Result<Vec<ProviderResult>> {
    let raw_result_count = raw_results.len();
    let results: Vec<_> = raw_results
        .into_iter()
        .filter_map(|raw| {
            let url = validated_web_url(&raw.url?)?;
            let title = bounded_non_empty(raw.title?, MAX_TITLE_CHARS)?;
            let snippet = raw
                .content
                .map(|content| sanitize_provider_text(&content, MAX_SNIPPET_CHARS))
                .unwrap_or_default();
            let mut result = ProviderResult::new(url, title, snippet);
            if let Some(full_text) = non_empty(raw.raw_content) {
                result = result.with_full_text(sanitize_provider_multiline_text(
                    &full_text,
                    MAX_FULL_TEXT_CHARS,
                ));
            }
            if let Some(score) = raw.score.filter(|score| score.is_finite()) {
                result = result.with_relevance_score(score.clamp(0.0, 1.0));
            }
            result.published_date = raw
                .published_date
                .map(|value| sanitize_provider_text(&value, MAX_DATE_CHARS))
                .filter(|value| !value.is_empty());
            result.favicon = raw.favicon.as_deref().and_then(validated_web_url);
            result.images = adapt_images(raw.images);
            Some(result)
        })
        .take(usize::from(max_results))
        .collect();
    if max_results > 0 && raw_result_count > 0 && results.is_empty() {
        return Err(ProviderError::new(
            PROVIDER_ID,
            ProviderErrorKind::InvalidResponse,
            "Tavily returned results without valid web URLs and titles",
        )
        .into());
    }
    Ok(results)
}

pub(super) fn adapt_images(images: Vec<TavilyImage>) -> Vec<SearchImage> {
    let mut adapted = Vec::new();
    for image in images.into_iter().filter_map(TavilyImage::adapt) {
        crate::result::merge_image(&mut adapted, image);
    }
    adapted
}

fn bounded_non_empty(value: String, max_chars: usize) -> Option<String> {
    let value = sanitize_provider_text(&value, max_chars);
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_time_conversion_never_saturates() {
        assert_eq!(FlexibleNumber::Number(1.25).seconds_to_ms(), Some(1_250));
        assert_eq!(
            FlexibleNumber::Number(18_446_744_073_709_551_616.0 / 1_000.0).seconds_to_ms(),
            None
        );
        assert_eq!(
            FlexibleNumber::String("inf".to_string()).seconds_to_ms(),
            None
        );
    }
}
