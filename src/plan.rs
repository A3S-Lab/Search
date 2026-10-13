//! Product cascade plan: source selection and tier ordering.
//!
//! The library owns the default API → HTTP/RSS → headless plan so embedders and
//! the CLI share one product policy. [`SearchCascade`] remains the structural
//! mechanism; this module is the reusable source plan that feeds it.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use crate::providers::BuiltinProvider;
use crate::SearchConfig;

#[cfg(feature = "headless")]
const DEFAULT_HTTP_TIER: [&str; 3] = ["wiki", "ddg", "bing"];
#[cfg(not(feature = "headless"))]
const DEFAULT_HTTP_TIER: [&str; 4] = ["wiki", "ddg", "brave", "bing"];
#[cfg(feature = "headless")]
const DEFAULT_HEADLESS_TIER: [&str; 2] = ["brave_browser", "bing_browser"];

/// Cascade transport class used by the default product plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineTier {
    /// Native search APIs (`SearchProvider` / `ProviderEngine`).
    Api,
    /// Conventional HTTP and RSS scrapers.
    HttpRss,
    /// Browser-rendered scrapers (Moli / Chrome / Lightpanda).
    Headless,
}

impl EngineTier {
    /// Stable receipt and diagnostics name for this tier.
    pub const fn receipt_name(self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::HttpRss => "http_rss",
            Self::Api => "api",
        }
    }

    /// Canonical CLI / config token for this tier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::HttpRss => "http-rss",
            Self::Headless => "headless",
        }
    }

    /// Default product order: native API, then HTTP/RSS, then headless.
    pub const fn default_order() -> [Self; 3] {
        [Self::Api, Self::HttpRss, Self::Headless]
    }
}

impl fmt::Display for EngineTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EngineTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "api" => Ok(Self::Api),
            "http-rss" | "http_rss" => Ok(Self::HttpRss),
            "headless" => Ok(Self::Headless),
            other => Err(format!(
                "unknown engine tier '{other}' (expected api, http-rss, or headless)"
            )),
        }
    }
}

/// Ordered API-first plan with conventional and browser fallbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineTierPlan {
    headless: Vec<String>,
    http_rss: Vec<String>,
    api: Vec<String>,
    unknown: Vec<String>,
    order: Vec<EngineTier>,
}

impl Default for EngineTierPlan {
    fn default() -> Self {
        Self {
            headless: Vec::new(),
            http_rss: Vec::new(),
            api: Vec::new(),
            unknown: Vec::new(),
            order: EngineTier::default_order().to_vec(),
        }
    }
}

impl EngineTierPlan {
    /// Builds a plan from an explicit selection, an ACL selection, or the
    /// built-in product defaults, in that precedence order.
    pub fn new(
        explicit: Option<&[String]>,
        config: Option<&SearchConfig>,
        order: Option<&[EngineTier]>,
    ) -> Result<Self, String> {
        let order = validated_order(order)?;
        let selected = match (explicit, config) {
            (Some(explicit), _) => explicit.to_vec(),
            (None, Some(config)) if !config.engines.is_empty() || !config.providers.is_empty() => {
                config
                    .enabled_sources()
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            }
            _ => {
                let mut plan = Self::builtin_default();
                plan.order = order;
                return Ok(plan);
            }
        };

        let mut plan = Self {
            order,
            ..Self::default()
        };
        for shortcut in selected {
            plan.add(&shortcut);
        }
        Ok(plan)
    }

    /// Returns the shipped product default (billed APIs excluded).
    pub fn builtin_default() -> Self {
        let mut plan = Self::default();
        #[cfg(feature = "headless")]
        for shortcut in DEFAULT_HEADLESS_TIER {
            plan.add(shortcut);
        }
        for shortcut in DEFAULT_HTTP_TIER {
            plan.add(shortcut);
        }
        for provider in BuiltinProvider::DEFAULT {
            plan.add(provider.id());
        }
        plan
    }

    fn add(&mut self, shortcut: &str) {
        let canonical = canonical_engine_shortcut(shortcut);
        let Some(tier) = engine_tier(&canonical) else {
            push_unique(&mut self.unknown, shortcut.trim().to_string());
            return;
        };
        let target = match tier {
            EngineTier::Headless => &mut self.headless,
            EngineTier::HttpRss => &mut self.http_rss,
            EngineTier::Api => &mut self.api,
        };
        push_unique(target, canonical);
    }

    /// Returns every recognized shortcut for diagnostics and proxy scoping.
    pub fn shortcuts(&self) -> Vec<String> {
        self.headless
            .iter()
            .chain(&self.http_rss)
            .chain(&self.api)
            .cloned()
            .collect()
    }

    /// Returns unrecognized explicit or ACL entries.
    pub fn unknown(&self) -> &[String] {
        &self.unknown
    }

    /// Returns whether no executable tier remains.
    pub fn is_empty(&self) -> bool {
        self.headless.is_empty() && self.http_rss.is_empty() && self.api.is_empty()
    }

    /// Headless-tier shortcuts in plan order.
    pub fn headless(&self) -> &[String] {
        &self.headless
    }

    /// HTTP/RSS-tier shortcuts in plan order.
    pub fn http_rss(&self) -> &[String] {
        &self.http_rss
    }

    /// Native API-tier shortcuts in plan order.
    pub fn api(&self) -> &[String] {
        &self.api
    }

    /// Tier execution order.
    pub fn order(&self) -> &[EngineTier] {
        &self.order
    }

    /// Non-empty tiers in the configured order.
    pub fn tiers(&self) -> Vec<(EngineTier, &[String])> {
        self.order
            .iter()
            .copied()
            .filter_map(|tier| {
                let shortcuts = match tier {
                    EngineTier::Headless => self.headless.as_slice(),
                    EngineTier::HttpRss => self.http_rss.as_slice(),
                    EngineTier::Api => self.api.as_slice(),
                };
                (!shortcuts.is_empty()).then_some((tier, shortcuts))
            })
            .collect()
    }
}

fn validated_order(order: Option<&[EngineTier]>) -> Result<Vec<EngineTier>, String> {
    let default = EngineTier::default_order();
    let order = order.unwrap_or(&default);
    if order.len() != default.len()
        || order.iter().copied().collect::<HashSet<_>>()
            != default.into_iter().collect::<HashSet<_>>()
    {
        return Err("tier order must contain api, http-rss, and headless exactly once".to_string());
    }
    Ok(order.to_vec())
}

fn canonical_engine_shortcut(shortcut: &str) -> String {
    match shortcut.trim().to_ascii_lowercase().as_str() {
        "duckduckgo" => "ddg".to_string(),
        "wikipedia" => "wiki".to_string(),
        "google" => "g".to_string(),
        "so360" => "360".to_string(),
        shortcut => shortcut.to_string(),
    }
}

fn engine_tier(shortcut: &str) -> Option<EngineTier> {
    if BuiltinProvider::from_id(shortcut).is_some() {
        return Some(EngineTier::Api);
    }
    match shortcut {
        "g" | "baidu" | "bing_browser" | "brave_browser" => Some(EngineTier::Headless),
        "ddg" | "brave" | "bing" | "wiki" | "sogou" | "360" | "bing_cn" => {
            Some(EngineTier::HttpRss)
        }
        _ => None,
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.iter().any(|current| current == &value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_plan_is_api_first_and_excludes_billed_providers() {
        let plan = EngineTierPlan::new(None, None, None).unwrap();
        let tiers = plan.tiers();

        assert_eq!(tiers[0].0, EngineTier::Api);
        assert_eq!(tiers[0].1, ["anysearch", "tavily"]);
        for billed in ["tinyfish", "bocha", "aliyun", "tencent", "firecrawl", "a3s"] {
            assert!(
                !plan.shortcuts().iter().any(|shortcut| shortcut == billed),
                "{billed} is billed and must stay out of the default plan"
            );
        }
        #[cfg(feature = "headless")]
        assert_eq!(tiers.last().unwrap().0, EngineTier::Headless);
        #[cfg(feature = "headless")]
        assert_eq!(plan.headless(), ["brave_browser", "bing_browser"]);
        #[cfg(feature = "headless")]
        assert_eq!(plan.http_rss(), ["wiki", "ddg", "bing"]);
        #[cfg(not(feature = "headless"))]
        assert_eq!(plan.http_rss(), ["wiki", "ddg", "brave", "bing"]);
        assert!(tiers.iter().any(|(tier, _)| *tier == EngineTier::HttpRss));
        assert!(plan.unknown().is_empty());
    }

    #[test]
    fn explicit_selection_is_canonicalized_deduplicated_and_not_expanded() {
        let selected = vec![
            "DuckDuckGo".to_string(),
            "ddg".to_string(),
            "GOOGLE".to_string(),
            "unknown".to_string(),
        ];
        let plan = EngineTierPlan::new(Some(&selected), None, None).unwrap();

        assert_eq!(plan.headless(), ["g"]);
        assert_eq!(plan.http_rss(), ["ddg"]);
        assert!(plan.api().is_empty());
        assert_eq!(plan.unknown(), &["unknown"]);
    }

    #[test]
    fn explicit_billed_provider_does_not_join_the_default_plan() {
        let selected = vec!["tinyfish".to_string()];
        let plan = EngineTierPlan::new(Some(&selected), None, None).unwrap();

        assert_eq!(plan.api(), ["tinyfish"]);
        assert!(plan.headless().is_empty());
        assert!(plan.http_rss().is_empty());
    }

    #[test]
    fn acl_selection_uses_only_enabled_sources() {
        let config = SearchConfig::parse(
            r#"
            engine "g" { enabled = true }
            engine "brave" { enabled = false }
            engine "wiki" { enabled = true }
            provider "anysearch" { enabled = true }
            provider "tavily" { enabled = false }
            "#,
        )
        .unwrap();
        let plan = EngineTierPlan::new(None, Some(&config), None).unwrap();

        assert_eq!(plan.headless(), ["g"]);
        assert_eq!(plan.http_rss(), ["wiki"]);
        assert_eq!(plan.api(), ["anysearch"]);
    }

    #[test]
    fn explicit_tier_order_is_a_complete_unique_permutation() {
        let order = [EngineTier::Api, EngineTier::HttpRss, EngineTier::Headless];
        let selected = ["anysearch", "wiki", "g"].map(str::to_string);
        let plan = EngineTierPlan::new(Some(&selected), None, Some(&order)).unwrap();
        let tiers = plan
            .tiers()
            .into_iter()
            .map(|(tier, _)| tier)
            .collect::<Vec<_>>();
        assert_eq!(tiers, order);

        let duplicate = [EngineTier::Api, EngineTier::Api, EngineTier::Headless];
        assert!(EngineTierPlan::new(None, None, Some(&duplicate)).is_err());
        assert!(EngineTierPlan::new(None, None, Some(&order[..2])).is_err());
    }

    #[test]
    fn tier_tokens_round_trip() {
        for tier in EngineTier::default_order() {
            assert_eq!(EngineTier::from_str(tier.as_str()).unwrap(), tier);
        }
        assert_eq!(
            EngineTier::from_str("http_rss").unwrap(),
            EngineTier::HttpRss
        );
    }
}
