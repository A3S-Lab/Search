//! Tavily Search API request wire type.

use serde::Serialize;

use super::types::{TavilyAnswer, TavilyRawContent, TavilySearchDepth, TavilyTopic};
use crate::TimeRange;

#[derive(Serialize)]
pub(super) struct TavilyRequest<'a> {
    pub(super) query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) search_depth: Option<TavilySearchDepth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) chunks_per_source: Option<u8>,
    pub(super) max_results: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) topic: Option<TavilyTopic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) time_range: Option<&'static str>,
    pub(super) include_answer: TavilyAnswer,
    pub(super) include_raw_content: TavilyRawContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) include_domains: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) exclude_domains: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) start_date: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) end_date: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) country: Option<&'a str>,
    pub(super) auto_parameters: bool,
    pub(super) exact_match: bool,
    pub(super) include_usage: bool,
    pub(super) include_images: bool,
    pub(super) include_image_descriptions: bool,
    pub(super) include_favicon: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) safe_search: Option<bool>,
}

pub(super) const fn time_range_name(time_range: TimeRange) -> &'static str {
    match time_range {
        TimeRange::Day => "day",
        TimeRange::Week => "week",
        TimeRange::Month => "month",
        TimeRange::Year => "year",
    }
}
