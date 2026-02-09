//! Page fetcher abstraction for retrieving HTML content.

use async_trait::async_trait;

use crate::Result;

/// Strategy for waiting until a page is considered fully loaded.
#[derive(Debug, Clone, Default)]
pub enum WaitStrategy {
    /// Wait for the page load event only.
    #[default]
    Load,
    /// Wait until network activity settles for the given duration.
    NetworkIdle {
        /// Milliseconds of network inactivity to wait for.
        idle_ms: u64,
    },
    /// Wait until a CSS selector matches an element on the page.
    Selector {
        /// CSS selector to wait for.
        css: String,
        /// Maximum time to wait in milliseconds before timing out.
        timeout_ms: u64,
    },
    /// Wait a fixed delay after the page load event.
    Delay {
        /// Milliseconds to wait after page load.
        ms: u64,
    },
}

/// Trait for fetching the full HTML content of a URL.
///
/// Implementations may use plain HTTP requests or a headless browser.
/// All configuration (user-agent, timeouts, wait strategy) is set at
/// construction time; `fetch` is a simple URL-in, HTML-out interface.
#[async_trait]
pub trait PageFetcher: Send + Sync {
    /// Fetches the HTML content of the given URL.
    async fn fetch(&self, url: &str) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_strategy_default() {
        let strategy = WaitStrategy::default();
        assert!(matches!(strategy, WaitStrategy::Load));
    }

    #[test]
    fn test_wait_strategy_network_idle() {
        let strategy = WaitStrategy::NetworkIdle { idle_ms: 500 };
        match strategy {
            WaitStrategy::NetworkIdle { idle_ms } => assert_eq!(idle_ms, 500),
            _ => panic!("Expected NetworkIdle"),
        }
    }

    #[test]
    fn test_wait_strategy_selector() {
        let strategy = WaitStrategy::Selector {
            css: "div.g".to_string(),
            timeout_ms: 5000,
        };
        match strategy {
            WaitStrategy::Selector { css, timeout_ms } => {
                assert_eq!(css, "div.g");
                assert_eq!(timeout_ms, 5000);
            }
            _ => panic!("Expected Selector"),
        }
    }

    #[test]
    fn test_wait_strategy_delay() {
        let strategy = WaitStrategy::Delay { ms: 1000 };
        match strategy {
            WaitStrategy::Delay { ms } => assert_eq!(ms, 1000),
            _ => panic!("Expected Delay"),
        }
    }

    #[test]
    fn test_wait_strategy_clone() {
        let original = WaitStrategy::Selector {
            css: "h1".to_string(),
            timeout_ms: 3000,
        };
        let cloned = original.clone();
        assert!(matches!(cloned, WaitStrategy::Selector { .. }));
    }

    #[test]
    fn test_wait_strategy_debug() {
        let strategy = WaitStrategy::Load;
        let debug = format!("{:?}", strategy);
        assert!(debug.contains("Load"));
    }

    #[test]
    fn test_wait_strategy_network_idle_debug() {
        let strategy = WaitStrategy::NetworkIdle { idle_ms: 500 };
        let debug = format!("{:?}", strategy);
        assert!(debug.contains("NetworkIdle"));
        assert!(debug.contains("500"));
    }

    #[test]
    fn test_wait_strategy_selector_debug() {
        let strategy = WaitStrategy::Selector {
            css: "div.g".to_string(),
            timeout_ms: 5000,
        };
        let debug = format!("{:?}", strategy);
        assert!(debug.contains("Selector"));
        assert!(debug.contains("div.g"));
    }

    #[test]
    fn test_wait_strategy_delay_debug() {
        let strategy = WaitStrategy::Delay { ms: 2000 };
        let debug = format!("{:?}", strategy);
        assert!(debug.contains("Delay"));
        assert!(debug.contains("2000"));
    }

    #[test]
    fn test_wait_strategy_clone_all_variants() {
        let load = WaitStrategy::Load;
        assert!(matches!(load.clone(), WaitStrategy::Load));

        let idle = WaitStrategy::NetworkIdle { idle_ms: 300 };
        if let WaitStrategy::NetworkIdle { idle_ms } = idle.clone() {
            assert_eq!(idle_ms, 300);
        } else {
            panic!("Expected NetworkIdle");
        }

        let delay = WaitStrategy::Delay { ms: 1500 };
        if let WaitStrategy::Delay { ms } = delay.clone() {
            assert_eq!(ms, 1500);
        } else {
            panic!("Expected Delay");
        }
    }
}
