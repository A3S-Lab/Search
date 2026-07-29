//! Frozen V1 encoding for complete typed search-query identities.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use super::{is_canonical_sha256, SearchCascadeReceiptError};
use crate::{EngineCategory, SafeSearch, SearchQuery, TimeRange};

const SEARCH_QUERY_BINDING_V1_DOMAIN: &[u8] = b"a3s/search-query-binding/v1\0";

/// A complete typed search query and its stable version-one identity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SearchQueryBindingV1 {
    /// Lowercase hexadecimal SHA-256 over the version-one query encoding.
    pub sha256: String,
    /// Exact typed query controls bound by `sha256`.
    pub value: SearchQuery,
}

impl SearchQueryBindingV1 {
    /// Binds every current [`SearchQuery`] control to a version-one identity.
    pub fn new(value: SearchQuery) -> Self {
        Self {
            sha256: search_query_sha256(&value),
            value,
        }
    }

    /// Recomputes and validates the query identity.
    pub fn validate(&self) -> Result<(), SearchCascadeReceiptError> {
        if !is_canonical_sha256(&self.sha256) || self.sha256 != search_query_sha256(&self.value) {
            return Err(SearchCascadeReceiptError::InvalidQueryDigest);
        }
        Ok(())
    }
}

impl Serialize for SearchQueryBindingV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SearchQueryBindingWireV1 {
            sha256: self.sha256.clone(),
            value: SearchQueryWireV1::from_query(&self.value),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SearchQueryBindingV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SearchQueryBindingWireV1::deserialize(deserializer)?;
        Ok(Self {
            sha256: wire.sha256,
            value: wire.value.into_query(),
        })
    }
}

fn search_query_sha256(query: &SearchQuery) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SEARCH_QUERY_BINDING_V1_DOMAIN);
    encode_search_query_v1(&mut hasher, query);
    format!("{:x}", hasher.finalize())
}

pub(super) fn encode_search_query_v1(hasher: &mut Sha256, query: &SearchQuery) {
    let SearchQuery {
        query,
        categories,
        language,
        safesearch,
        page,
        time_range,
        engines,
    } = query;

    update_label(hasher, b"query");
    update_bytes(hasher, query.as_bytes());

    update_label(hasher, b"categories");
    update_length(hasher, categories.len());
    for category in categories {
        hasher.update([engine_category_tag(*category)]);
    }

    update_label(hasher, b"language");
    match language {
        Some(language) => {
            hasher.update([1]);
            update_bytes(hasher, language.as_bytes());
        }
        None => hasher.update([0]),
    }

    update_label(hasher, b"safesearch");
    hasher.update([safe_search_tag(*safesearch)]);

    update_label(hasher, b"page");
    hasher.update(page.to_be_bytes());

    update_label(hasher, b"time_range");
    match time_range {
        Some(range) => hasher.update([1, time_range_tag(*range)]),
        None => hasher.update([0]),
    }

    update_label(hasher, b"engines");
    update_length(hasher, engines.len());
    for engine in engines {
        update_bytes(hasher, engine.as_bytes());
    }
}

fn update_label(hasher: &mut Sha256, label: &[u8]) {
    update_bytes(hasher, label);
}

fn update_bytes(hasher: &mut Sha256, value: &[u8]) {
    update_length(hasher, value.len());
    hasher.update(value);
}

fn update_length(hasher: &mut Sha256, length: usize) {
    hasher.update((length as u128).to_be_bytes());
}

fn engine_category_tag(category: EngineCategory) -> u8 {
    match category {
        EngineCategory::General => 0,
        EngineCategory::Images => 1,
        EngineCategory::Videos => 2,
        EngineCategory::News => 3,
        EngineCategory::Maps => 4,
        EngineCategory::Music => 5,
        EngineCategory::Files => 6,
        EngineCategory::Science => 7,
        EngineCategory::Social => 8,
    }
}

fn safe_search_tag(value: SafeSearch) -> u8 {
    match value {
        SafeSearch::Off => 0,
        SafeSearch::Moderate => 1,
        SafeSearch::Strict => 2,
    }
}

fn time_range_tag(value: TimeRange) -> u8 {
    match value {
        TimeRange::Day => 0,
        TimeRange::Week => 1,
        TimeRange::Month => 2,
        TimeRange::Year => 3,
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQueryBindingWireV1 {
    sha256: String,
    value: SearchQueryWireV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQueryWireV1 {
    query: String,
    categories: Vec<EngineCategoryWireV1>,
    language: Option<String>,
    safesearch: SafeSearchWireV1,
    page: u32,
    time_range: Option<TimeRangeWireV1>,
    engines: Vec<String>,
}

impl SearchQueryWireV1 {
    fn from_query(value: &SearchQuery) -> Self {
        let SearchQuery {
            query,
            categories,
            language,
            safesearch,
            page,
            time_range,
            engines,
        } = value;
        Self {
            query: query.clone(),
            categories: categories.iter().copied().map(Into::into).collect(),
            language: language.clone(),
            safesearch: (*safesearch).into(),
            page: *page,
            time_range: time_range.map(Into::into),
            engines: engines.clone(),
        }
    }

    fn into_query(self) -> SearchQuery {
        SearchQuery {
            query: self.query,
            categories: self.categories.into_iter().map(Into::into).collect(),
            language: self.language,
            safesearch: self.safesearch.into(),
            page: self.page,
            time_range: self.time_range.map(Into::into),
            engines: self.engines,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EngineCategoryWireV1 {
    General,
    Images,
    Videos,
    News,
    Maps,
    Music,
    Files,
    Science,
    Social,
}

impl From<EngineCategory> for EngineCategoryWireV1 {
    fn from(value: EngineCategory) -> Self {
        match value {
            EngineCategory::General => Self::General,
            EngineCategory::Images => Self::Images,
            EngineCategory::Videos => Self::Videos,
            EngineCategory::News => Self::News,
            EngineCategory::Maps => Self::Maps,
            EngineCategory::Music => Self::Music,
            EngineCategory::Files => Self::Files,
            EngineCategory::Science => Self::Science,
            EngineCategory::Social => Self::Social,
        }
    }
}

impl From<EngineCategoryWireV1> for EngineCategory {
    fn from(value: EngineCategoryWireV1) -> Self {
        match value {
            EngineCategoryWireV1::General => Self::General,
            EngineCategoryWireV1::Images => Self::Images,
            EngineCategoryWireV1::Videos => Self::Videos,
            EngineCategoryWireV1::News => Self::News,
            EngineCategoryWireV1::Maps => Self::Maps,
            EngineCategoryWireV1::Music => Self::Music,
            EngineCategoryWireV1::Files => Self::Files,
            EngineCategoryWireV1::Science => Self::Science,
            EngineCategoryWireV1::Social => Self::Social,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum SafeSearchWireV1 {
    Off,
    Moderate,
    Strict,
}

impl From<SafeSearch> for SafeSearchWireV1 {
    fn from(value: SafeSearch) -> Self {
        match value {
            SafeSearch::Off => Self::Off,
            SafeSearch::Moderate => Self::Moderate,
            SafeSearch::Strict => Self::Strict,
        }
    }
}

impl From<SafeSearchWireV1> for SafeSearch {
    fn from(value: SafeSearchWireV1) -> Self {
        match value {
            SafeSearchWireV1::Off => Self::Off,
            SafeSearchWireV1::Moderate => Self::Moderate,
            SafeSearchWireV1::Strict => Self::Strict,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum TimeRangeWireV1 {
    Day,
    Week,
    Month,
    Year,
}

impl From<TimeRange> for TimeRangeWireV1 {
    fn from(value: TimeRange) -> Self {
        match value {
            TimeRange::Day => Self::Day,
            TimeRange::Week => Self::Week,
            TimeRange::Month => Self::Month,
            TimeRange::Year => Self::Year,
        }
    }
}

impl From<TimeRangeWireV1> for TimeRange {
    fn from(value: TimeRangeWireV1) -> Self {
        match value {
            TimeRangeWireV1::Day => Self::Day,
            TimeRangeWireV1::Week => Self::Week,
            TimeRangeWireV1::Month => Self::Month,
            TimeRangeWireV1::Year => Self::Year,
        }
    }
}
