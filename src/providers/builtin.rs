//! Registry for providers shipped with `a3s-search`.

use super::{
    AliyunProvider, AnySearchProvider, BochaProvider, FirecrawlProvider, ProviderEngine,
    TavilyProvider, TencentProvider, TinyFishProvider,
};
use crate::Result;

/// Native API providers included in the CLI and library distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BuiltinProvider {
    /// AnySearch, with optional authenticated or anonymous access.
    AnySearch,
    /// Tavily Search API, with keyless or authenticated access.
    Tavily,
    /// TinyFish Search API. Requires `TINYFISH_API_KEY`.
    TinyFish,
    /// Bocha Web Search API. Requires `BOCHA_API_KEY`.
    Bocha,
    /// Alibaba Cloud IQS Unified Search. Requires `ALIYUN_IQS_API_KEY`.
    Aliyun,
    /// Tencent Cloud SearchPro. Requires `TENCENTCLOUD_WSA_APIKEY`.
    Tencent,
    /// Firecrawl Search API. Requires `FIRECRAWL_API_KEY`.
    Firecrawl,
}

impl BuiltinProvider {
    /// All built-in providers in stable display order.
    pub const ALL: [Self; 7] = [
        Self::AnySearch,
        Self::Tavily,
        Self::TinyFish,
        Self::Bocha,
        Self::Aliyun,
        Self::Tencent,
        Self::Firecrawl,
    ];

    /// Providers included in the default CLI plan.
    ///
    /// Credential-required AI search APIs stay explicit so a missing key does
    /// not fail every search and an ambient credential does not start billing.
    pub const DEFAULT: [Self; 2] = [Self::AnySearch, Self::Tavily];

    /// Resolves a stable provider identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "anysearch" => Some(Self::AnySearch),
            "tavily" => Some(Self::Tavily),
            "tinyfish" => Some(Self::TinyFish),
            "bocha" => Some(Self::Bocha),
            "aliyun" => Some(Self::Aliyun),
            "tencent" => Some(Self::Tencent),
            "firecrawl" => Some(Self::Firecrawl),
            _ => None,
        }
    }

    /// Returns the stable provider identifier.
    pub const fn id(self) -> &'static str {
        match self {
            Self::AnySearch => "anysearch",
            Self::Tavily => "tavily",
            Self::TinyFish => "tinyfish",
            Self::Bocha => "bocha",
            Self::Aliyun => "aliyun",
            Self::Tencent => "tencent",
            Self::Firecrawl => "firecrawl",
        }
    }

    /// Creates an engine using the provider's documented environment defaults.
    pub fn create_engine(self) -> Result<ProviderEngine> {
        match self {
            Self::AnySearch => Ok(ProviderEngine::new(AnySearchProvider::from_env()?)),
            Self::Tavily => Ok(ProviderEngine::new(TavilyProvider::from_env()?)),
            Self::TinyFish => Ok(ProviderEngine::new(TinyFishProvider::from_env()?)),
            Self::Bocha => Ok(ProviderEngine::new(BochaProvider::from_env()?)),
            Self::Aliyun => Ok(ProviderEngine::new(AliyunProvider::from_env()?)),
            Self::Tencent => Ok(ProviderEngine::new(TencentProvider::from_env()?)),
            Self::Firecrawl => Ok(ProviderEngine::new(FirecrawlProvider::from_env()?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    #[test]
    fn registry_ids_are_stable_and_match_engine_descriptors() {
        let ids: Vec<_> = BuiltinProvider::ALL
            .iter()
            .copied()
            .map(BuiltinProvider::id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "anysearch",
                "tavily",
                "tinyfish",
                "bocha",
                "aliyun",
                "tencent",
                "firecrawl"
            ]
        );
        let defaults: Vec<_> = BuiltinProvider::DEFAULT
            .iter()
            .copied()
            .map(BuiltinProvider::id)
            .collect();
        assert_eq!(defaults, vec!["anysearch", "tavily"]);

        for provider in BuiltinProvider::ALL {
            assert_eq!(provider.create_engine().unwrap().shortcut(), provider.id());
            assert_eq!(BuiltinProvider::from_id(provider.id()), Some(provider));
        }
        assert_eq!(BuiltinProvider::from_id("unknown"), None);
    }
}
