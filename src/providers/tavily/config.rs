//! Typed Tavily provider configuration and cross-field validation.

use std::collections::BTreeSet;

use url::{Host, Url};

use super::super::http::validate_provider_endpoint;
use super::super::{CredentialSource, ProviderHttpConfig};
use super::types::{
    TavilyAnswer, TavilyCountry, TavilyDate, TavilyRawContent, TavilySearchDepth, TavilyTopic,
};
use super::PROVIDER_ID;
use crate::{ProviderError, ProviderErrorKind, Result, SearchError};

pub(super) const DEFAULT_ENDPOINT: &str = "https://api.tavily.com/search";

/// Typed Tavily request defaults and credentials.
#[derive(Debug, Clone)]
pub struct TavilyConfig {
    pub(super) endpoint: Url,
    pub(super) api_key: CredentialSource,
    pub(super) project: CredentialSource,
    pub(super) search_depth: TavilySearchDepth,
    pub(super) search_depth_explicit: bool,
    pub(super) chunks_per_source: Option<u8>,
    pub(super) max_results: u8,
    pub(super) topic: TavilyTopic,
    pub(super) topic_explicit: bool,
    pub(super) include_answer: TavilyAnswer,
    pub(super) include_raw_content: TavilyRawContent,
    pub(super) include_domains: Vec<String>,
    pub(super) exclude_domains: Vec<String>,
    pub(super) start_date: Option<TavilyDate>,
    pub(super) end_date: Option<TavilyDate>,
    pub(super) country: Option<TavilyCountry>,
    pub(super) auto_parameters: bool,
    pub(super) exact_match: bool,
    pub(super) include_usage: bool,
    pub(super) include_images: bool,
    pub(super) include_image_descriptions: bool,
    pub(super) include_favicon: bool,
    pub(super) safe_search: Option<bool>,
    pub(super) http: ProviderHttpConfig,
}

impl TavilyConfig {
    /// Creates the default Tavily configuration.
    ///
    /// `TAVILY_API_KEY` is optional. When it is absent, requests use Tavily's
    /// documented keyless access mode. `TAVILY_PROJECT` is sent only with an
    /// authenticated request.
    pub fn new() -> Result<Self> {
        let endpoint = Url::parse(DEFAULT_ENDPOINT).map_err(|_| {
            ProviderError::new(
                PROVIDER_ID,
                ProviderErrorKind::InvalidRequest,
                "built-in Tavily endpoint is invalid",
            )
        })?;
        Ok(Self {
            endpoint,
            api_key: CredentialSource::environment("TAVILY_API_KEY"),
            project: CredentialSource::environment("TAVILY_PROJECT"),
            search_depth: TavilySearchDepth::Basic,
            search_depth_explicit: false,
            chunks_per_source: None,
            max_results: 5,
            topic: TavilyTopic::General,
            topic_explicit: false,
            include_answer: TavilyAnswer::None,
            include_raw_content: TavilyRawContent::None,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            start_date: None,
            end_date: None,
            country: None,
            auto_parameters: false,
            exact_match: false,
            include_usage: false,
            include_images: false,
            include_image_descriptions: false,
            include_favicon: false,
            safe_search: None,
            http: ProviderHttpConfig::default(),
        })
    }

    /// Replaces the API endpoint.
    pub fn with_endpoint(mut self, endpoint: Url) -> Result<Self> {
        validate_provider_endpoint(PROVIDER_ID, &endpoint)?;
        self.endpoint = endpoint;
        Ok(self)
    }

    /// Replaces the API-key source.
    pub fn with_api_key(mut self, api_key: CredentialSource) -> Self {
        self.api_key = api_key;
        self
    }

    /// Replaces the optional project source.
    pub fn with_project(mut self, project: CredentialSource) -> Self {
        self.project = project;
        self
    }

    /// Sets search depth.
    pub fn with_search_depth(mut self, search_depth: TavilySearchDepth) -> Self {
        self.search_depth = search_depth;
        self.search_depth_explicit = true;
        self
    }

    /// Sets advanced-search chunks per source in the documented `1..=3` range.
    pub fn with_chunks_per_source(mut self, chunks_per_source: u8) -> Result<Self> {
        if !(1..=3).contains(&chunks_per_source) {
            return Err(invalid_config(
                "Tavily chunks_per_source must be between 1 and 3",
            ));
        }
        self.chunks_per_source = Some(chunks_per_source);
        Ok(self)
    }

    /// Sets maximum results in the documented `0..=20` range.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if max_results > 20 {
            return Err(invalid_config(
                "Tavily max_results must be between 0 and 20",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Sets the search topic.
    pub fn with_topic(mut self, topic: TavilyTopic) -> Self {
        self.topic = topic;
        self.topic_explicit = true;
        self
    }

    /// Sets direct-answer mode.
    pub fn with_answer(mut self, include_answer: TavilyAnswer) -> Self {
        self.include_answer = include_answer;
        self
    }

    /// Sets source-content mode.
    pub fn with_raw_content(mut self, include_raw_content: TavilyRawContent) -> Self {
        self.include_raw_content = include_raw_content;
        self
    }

    /// Sets the domain allow list.
    pub fn with_include_domains<I, S>(mut self, domains: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.include_domains = validated_domains(domains)?;
        if self.include_domains.len() > 300 {
            return Err(invalid_config(
                "Tavily include_domains accepts at most 300 domains",
            ));
        }
        Ok(self)
    }

    /// Sets the domain deny list.
    pub fn with_exclude_domains<I, S>(mut self, domains: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exclude_domains = validated_domains(domains)?;
        if self.exclude_domains.len() > 150 {
            return Err(invalid_config(
                "Tavily exclude_domains accepts at most 150 domains",
            ));
        }
        Ok(self)
    }

    /// Sets the inclusive lower date bound.
    pub fn with_start_date(mut self, start_date: TavilyDate) -> Self {
        self.start_date = Some(start_date);
        self
    }

    /// Sets the inclusive upper date bound.
    pub fn with_end_date(mut self, end_date: TavilyDate) -> Self {
        self.end_date = Some(end_date);
        self
    }

    /// Boosts results from a documented country.
    pub fn with_country(mut self, country: TavilyCountry) -> Self {
        self.country = Some(country);
        self
    }

    /// Enables Tavily automatic parameter selection.
    pub fn with_auto_parameters(mut self, auto_parameters: bool) -> Self {
        self.auto_parameters = auto_parameters;
        self
    }

    /// Enables exact query matching.
    pub fn with_exact_match(mut self, exact_match: bool) -> Self {
        self.exact_match = exact_match;
        self
    }

    /// Controls whether Tavily usage details are requested.
    pub fn with_include_usage(mut self, include_usage: bool) -> Self {
        self.include_usage = include_usage;
        self
    }

    /// Controls whether query and per-result images are requested.
    pub fn with_include_images(mut self, include_images: bool) -> Self {
        self.include_images = include_images;
        self
    }

    /// Controls whether requested images include descriptions.
    pub fn with_image_descriptions(mut self, include_image_descriptions: bool) -> Self {
        self.include_image_descriptions = include_image_descriptions;
        self
    }

    /// Controls whether each result includes a favicon.
    pub fn with_favicon(mut self, include_favicon: bool) -> Self {
        self.include_favicon = include_favicon;
        self
    }

    /// Explicitly enables or disables Tavily enterprise safe search.
    pub fn with_safe_search(mut self, safe_search: bool) -> Self {
        self.safe_search = Some(safe_search);
        self
    }

    /// Replaces shared HTTP safety limits.
    pub fn with_http_config(mut self, http: ProviderHttpConfig) -> Self {
        self.http = http;
        self
    }

    /// Returns the configured endpoint.
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Returns the maximum result count.
    pub const fn max_results(&self) -> u8 {
        self.max_results
    }

    /// Returns the configured search depth.
    pub const fn search_depth(&self) -> TavilySearchDepth {
        self.search_depth
    }

    /// Returns the configured topic.
    pub const fn topic(&self) -> TavilyTopic {
        self.topic
    }

    /// Returns normalized allowed domains.
    pub fn include_domains(&self) -> &[String] {
        &self.include_domains
    }

    /// Returns normalized excluded domains.
    pub fn exclude_domains(&self) -> &[String] {
        &self.exclude_domains
    }

    pub(super) fn request_search_depth(&self, safe_search: bool) -> Option<TavilySearchDepth> {
        (!self.auto_parameters
            || self.search_depth_explicit
            || self.chunks_per_source.is_some()
            || safe_search)
            .then_some(self.search_depth)
    }

    pub(super) fn request_topic(&self) -> Option<TavilyTopic> {
        (!self.auto_parameters || self.topic_explicit || self.country.is_some())
            .then_some(self.topic)
    }

    pub(super) fn validate(&self) -> Result<()> {
        if self.chunks_per_source.is_some() && self.search_depth != TavilySearchDepth::Advanced {
            return Err(invalid_config(
                "Tavily chunks_per_source is only valid for advanced search",
            ));
        }
        let include: BTreeSet<_> = self.include_domains.iter().collect();
        if self
            .exclude_domains
            .iter()
            .any(|domain| include.contains(domain))
        {
            return Err(invalid_config(
                "a Tavily domain cannot be both included and excluded",
            ));
        }
        if self
            .start_date
            .as_ref()
            .zip(self.end_date.as_ref())
            .is_some_and(|(start, end)| start > end)
        {
            return Err(invalid_config(
                "Tavily start_date must not be later than end_date",
            ));
        }
        if self.country.is_some() && self.topic != TavilyTopic::General {
            return Err(invalid_config(
                "Tavily country is only valid for the general topic",
            ));
        }
        if self.include_image_descriptions && !self.include_images {
            return Err(invalid_config(
                "Tavily include_image_descriptions requires include_images",
            ));
        }
        if self.safe_search == Some(true) && !self.search_depth.supports_safe_search() {
            return Err(invalid_config(
                "Tavily safe_search is not supported for fast or ultra-fast depth",
            ));
        }
        Ok(())
    }
}

fn validated_domains<I, S>(domains: I) -> Result<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut validated = BTreeSet::new();
    for domain in domains {
        let domain = normalized_domain(&domain.into()).ok_or_else(|| {
            invalid_config("Tavily domains must be valid bare DNS names without paths or schemes")
        })?;
        validated.insert(domain);
    }
    Ok(validated.into_iter().collect())
}

fn normalized_domain(value: &str) -> Option<String> {
    let Host::Domain(domain) = Host::parse(value.trim()).ok()? else {
        return None;
    };
    let valid = !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        });
    valid.then_some(domain)
}

fn invalid_config(message: &str) -> SearchError {
    ProviderError::new(PROVIDER_ID, ProviderErrorKind::InvalidRequest, message).into()
}
