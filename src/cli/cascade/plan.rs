//! CLI engine selection and tier planning.

use std::collections::HashSet;

use a3s_search::{providers::BuiltinProvider, SearchConfig};

#[cfg(feature = "headless")]
const DEFAULT_HTTP_TIER: [&str; 3] = ["wiki", "ddg", "bing"];
#[cfg(not(feature = "headless"))]
const DEFAULT_HTTP_TIER: [&str; 4] = ["wiki", "ddg", "brave", "bing"];
#[cfg(feature = "headless")]
const DEFAULT_HEADLESS_TIER: [&str; 2] = ["brave_browser", "bing_browser"];

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EngineTier {
    Headless,
    HttpRss,
    Api,
}

impl EngineTier {
    pub(super) const fn receipt_name(self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::HttpRss => "http_rss",
            Self::Api => "api",
        }
    }
}

/// Ordered API-first plan with conventional and browser fallbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EngineTierPlan {
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
            order: vec![EngineTier::Api, EngineTier::HttpRss, EngineTier::Headless],
        }
    }
}

impl EngineTierPlan {
    /// Builds a plan from an explicit CLI selection, an ACL selection, or the
    /// generic built-in defaults, in that precedence order.
    pub(crate) fn new(
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

    fn builtin_default() -> Self {
        let mut plan = Self::default();
        #[cfg(feature = "headless")]
        for shortcut in DEFAULT_HEADLESS_TIER {
            plan.add(shortcut);
        }
        for shortcut in DEFAULT_HTTP_TIER {
            plan.add(shortcut);
        }
        for provider in BuiltinProvider::ALL {
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
    pub(crate) fn shortcuts(&self) -> Vec<String> {
        self.headless
            .iter()
            .chain(&self.http_rss)
            .chain(&self.api)
            .cloned()
            .collect()
    }

    /// Returns unrecognized explicit or ACL entries.
    pub(crate) fn unknown(&self) -> &[String] {
        &self.unknown
    }

    /// Returns whether no executable tier remains.
    pub(crate) fn is_empty(&self) -> bool {
        self.headless.is_empty() && self.http_rss.is_empty() && self.api.is_empty()
    }

    pub(super) fn tiers(&self) -> Vec<(EngineTier, &[String])> {
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
    let default = [EngineTier::Api, EngineTier::HttpRss, EngineTier::Headless];
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
    fn default_plan_is_api_first_with_chrome_as_the_final_transport() {
        let plan = EngineTierPlan::new(None, None, None).unwrap();
        let tiers = plan.tiers();

        assert_eq!(tiers[0].0, EngineTier::Api);
        assert_eq!(tiers[0].1, ["anysearch", "tavily"]);
        #[cfg(feature = "headless")]
        assert_eq!(tiers.last().unwrap().0, EngineTier::Headless);
        #[cfg(feature = "headless")]
        assert_eq!(plan.headless, ["brave_browser", "bing_browser"]);
        #[cfg(feature = "headless")]
        assert_eq!(plan.http_rss, ["wiki", "ddg", "bing"]);
        #[cfg(not(feature = "headless"))]
        assert_eq!(plan.http_rss, ["wiki", "ddg", "brave", "bing"]);
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

        assert_eq!(plan.headless, vec!["g"]);
        assert_eq!(plan.http_rss, vec!["ddg"]);
        assert!(plan.api.is_empty());
        assert_eq!(plan.unknown(), &["unknown"]);
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

        assert_eq!(plan.headless, vec!["g"]);
        assert_eq!(plan.http_rss, vec!["wiki"]);
        assert_eq!(plan.api, vec!["anysearch"]);
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
}
