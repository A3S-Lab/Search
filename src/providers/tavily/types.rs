//! Tavily request value types.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{ProviderError, ProviderErrorKind, Result};

const PROVIDER_ID: &str = "tavily";

/// Tavily search depth.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TavilySearchDepth {
    /// Highest relevance with increased latency.
    Advanced,
    /// Balanced relevance and latency.
    #[default]
    Basic,
    /// Lower-latency multi-snippet search.
    Fast,
    /// Minimum-latency search.
    UltraFast,
}

impl TavilySearchDepth {
    /// Returns the wire-format depth.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Advanced => "advanced",
            Self::Basic => "basic",
            Self::Fast => "fast",
            Self::UltraFast => "ultra-fast",
        }
    }

    pub(crate) const fn supports_safe_search(self) -> bool {
        matches!(self, Self::Advanced | Self::Basic)
    }
}

/// Tavily search topic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TavilyTopic {
    /// General-purpose search.
    #[default]
    General,
    /// News search.
    News,
    /// Finance search.
    Finance,
}

impl TavilyTopic {
    /// Returns the wire-format topic.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::News => "news",
            Self::Finance => "finance",
        }
    }
}

/// Tavily direct-answer mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TavilyAnswer {
    /// Do not request an answer.
    #[default]
    None,
    /// Request a concise answer.
    Basic,
    /// Request an advanced answer.
    Advanced,
}

impl Serialize for TavilyAnswer {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::None => serializer.serialize_bool(false),
            Self::Basic => serializer.serialize_str("basic"),
            Self::Advanced => serializer.serialize_str("advanced"),
        }
    }
}

/// Tavily raw-content mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TavilyRawContent {
    /// Do not request source content.
    None,
    /// Request Markdown source content.
    Markdown,
    /// Request plain-text source content.
    #[default]
    Text,
}

impl Serialize for TavilyRawContent {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::None => serializer.serialize_bool(false),
            Self::Markdown => serializer.serialize_str("markdown"),
            Self::Text => serializer.serialize_str("text"),
        }
    }
}

/// Validated Tavily date in `YYYY-MM-DD` format.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct TavilyDate(String);

impl TavilyDate {
    /// Parses and validates a calendar date.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into().trim().to_string();
        if !valid_date(&value) {
            return Err(invalid_config(
                "Tavily dates must be valid calendar dates in YYYY-MM-DD format",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the validated date.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TavilyDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TavilyDate {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated country accepted by Tavily's `country` boost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TavilyCountry(String);

impl TavilyCountry {
    /// Parses and validates an official Tavily country name.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value
            .into()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        if SUPPORTED_COUNTRIES.binary_search(&value.as_str()).is_err() {
            return Err(invalid_config(
                "Tavily country must be one of the documented country names",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the normalized country name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TavilyCountry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TavilyCountry {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

fn valid_date(value: &str) -> bool {
    if value.len() != 10
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
        || !value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }
    let Ok(year) = value[0..4].parse::<u16>() else {
        return false;
    };
    let Ok(month) = value[5..7].parse::<u8>() else {
        return false;
    };
    let Ok(day) = value[8..10].parse::<u8>() else {
        return false;
    };
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap_year => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

fn invalid_config(message: &str) -> crate::SearchError {
    ProviderError::new(PROVIDER_ID, ProviderErrorKind::InvalidRequest, message).into()
}

// Keep this sorted for binary search. Source:
// https://docs.tavily.com/documentation/api-reference/endpoint/search
const SUPPORTED_COUNTRIES: &[&str] = &[
    "afghanistan",
    "albania",
    "algeria",
    "andorra",
    "angola",
    "argentina",
    "armenia",
    "australia",
    "austria",
    "azerbaijan",
    "bahamas",
    "bahrain",
    "bangladesh",
    "barbados",
    "belarus",
    "belgium",
    "belize",
    "benin",
    "bhutan",
    "bolivia",
    "bosnia and herzegovina",
    "botswana",
    "brazil",
    "brunei",
    "bulgaria",
    "burkina faso",
    "burundi",
    "cambodia",
    "cameroon",
    "canada",
    "cape verde",
    "central african republic",
    "chad",
    "chile",
    "china",
    "colombia",
    "comoros",
    "congo",
    "costa rica",
    "croatia",
    "cuba",
    "cyprus",
    "czech republic",
    "denmark",
    "djibouti",
    "dominican republic",
    "ecuador",
    "egypt",
    "el salvador",
    "equatorial guinea",
    "eritrea",
    "estonia",
    "ethiopia",
    "fiji",
    "finland",
    "france",
    "gabon",
    "gambia",
    "georgia",
    "germany",
    "ghana",
    "greece",
    "guatemala",
    "guinea",
    "haiti",
    "honduras",
    "hungary",
    "iceland",
    "india",
    "indonesia",
    "iran",
    "iraq",
    "ireland",
    "israel",
    "italy",
    "jamaica",
    "japan",
    "jordan",
    "kazakhstan",
    "kenya",
    "kuwait",
    "kyrgyzstan",
    "latvia",
    "lebanon",
    "lesotho",
    "liberia",
    "libya",
    "liechtenstein",
    "lithuania",
    "luxembourg",
    "madagascar",
    "malawi",
    "malaysia",
    "maldives",
    "mali",
    "malta",
    "mauritania",
    "mauritius",
    "mexico",
    "moldova",
    "monaco",
    "mongolia",
    "montenegro",
    "morocco",
    "mozambique",
    "myanmar",
    "namibia",
    "nepal",
    "netherlands",
    "new zealand",
    "nicaragua",
    "niger",
    "nigeria",
    "north korea",
    "north macedonia",
    "norway",
    "oman",
    "pakistan",
    "panama",
    "papua new guinea",
    "paraguay",
    "peru",
    "philippines",
    "poland",
    "portugal",
    "qatar",
    "romania",
    "russia",
    "rwanda",
    "saudi arabia",
    "senegal",
    "serbia",
    "singapore",
    "slovakia",
    "slovenia",
    "somalia",
    "south africa",
    "south korea",
    "south sudan",
    "spain",
    "sri lanka",
    "sudan",
    "sweden",
    "switzerland",
    "syria",
    "taiwan",
    "tajikistan",
    "tanzania",
    "thailand",
    "togo",
    "trinidad and tobago",
    "tunisia",
    "turkey",
    "turkmenistan",
    "uganda",
    "ukraine",
    "united arab emirates",
    "united kingdom",
    "united states",
    "uruguay",
    "uzbekistan",
    "venezuela",
    "vietnam",
    "yemen",
    "zambia",
    "zimbabwe",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_real_calendar_dates() {
        assert_eq!(
            TavilyDate::new("2024-02-29").unwrap().as_str(),
            "2024-02-29"
        );
        assert!(TavilyDate::new("2025-02-29").is_err());
        assert!(TavilyDate::new("2025-13-01").is_err());
        assert!(TavilyDate::new("25-01-01").is_err());
    }

    #[test]
    fn date_deserialization_preserves_validation() {
        let date: TavilyDate = serde_json::from_str("\"2024-02-29\"").unwrap();

        assert_eq!(date.as_str(), "2024-02-29");
        assert!(serde_json::from_str::<TavilyDate>("\"2025-02-29\"").is_err());
    }

    #[test]
    fn validates_and_normalizes_documented_countries() {
        assert_eq!(
            TavilyCountry::new("  United   States ").unwrap().as_str(),
            "united states"
        );
        assert!(TavilyCountry::new("atlantis").is_err());
    }

    #[test]
    fn country_deserialization_preserves_validation_and_normalization() {
        let country: TavilyCountry = serde_json::from_str("\"  United   States \"").unwrap();

        assert_eq!(country.as_str(), "united states");
        assert!(serde_json::from_str::<TavilyCountry>("\"atlantis\"").is_err());
    }
}
