//! Registry for providers shipped with `a3s-search`.

use super::{AnySearchProvider, ProviderEngine, TavilyProvider};
use crate::Result;

/// Native API providers included in the CLI and library distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BuiltinProvider {
    /// AnySearch, with optional authenticated or anonymous access.
    AnySearch,
    /// Tavily Search API, with keyless or authenticated access.
    Tavily,
}

impl BuiltinProvider {
    /// All built-in providers in stable display order.
    pub const ALL: [Self; 2] = [Self::AnySearch, Self::Tavily];

    /// Resolves a stable provider identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "anysearch" => Some(Self::AnySearch),
            "tavily" => Some(Self::Tavily),
            _ => None,
        }
    }

    /// Returns the stable provider identifier.
    pub const fn id(self) -> &'static str {
        match self {
            Self::AnySearch => "anysearch",
            Self::Tavily => "tavily",
        }
    }

    /// Creates an engine using the provider's documented environment defaults.
    pub fn create_engine(self) -> Result<ProviderEngine> {
        match self {
            Self::AnySearch => Ok(ProviderEngine::new(AnySearchProvider::from_env()?)),
            Self::Tavily => Ok(ProviderEngine::new(TavilyProvider::from_env()?)),
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
        assert_eq!(ids, vec!["anysearch", "tavily"]);

        for provider in BuiltinProvider::ALL {
            assert_eq!(provider.create_engine().unwrap().shortcut(), provider.id());
            assert_eq!(BuiltinProvider::from_id(provider.id()), Some(provider));
        }
        assert_eq!(BuiltinProvider::from_id("unknown"), None);
    }
}
